use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::{AppContext as _, AsyncApp, Context, EventEmitter, WeakEntity};

use adb_core::{AdbClient, Device, DeviceState, spawn_device_watcher};

/// Emitted after any coherent change to the store's snapshot or selection.
#[derive(Clone, Debug)]
pub enum DeviceStoreEvent {
    Changed,
}

enum BridgeMsg {
    Snapshot(Vec<Device>),
    /// No snapshot within the poll window; a chance to notice a released store.
    Idle,
    /// The watcher exited and dropped its sender.
    Disconnected,
}

/// The device list live from `adb track-devices`, plus the current selection.
///
/// Owns the watcher's receiver: the bridge task polls it with a short timeout
/// so that when the store entity is released the task ends, the receiver
/// drops, and the watcher kills its adb child (see `adb_core::track`).
pub struct DeviceStore {
    client: AdbClient,
    devices: Vec<Device>,
    selected_serial: Option<String>,
    /// Serials whose model was already fetched (or is being fetched) —
    /// enrichment runs at most once per device.
    enriched: HashSet<String>,
}

impl DeviceStore {
    /// Start the part-02 watcher and bridge its snapshots into GPUI.
    pub fn new(client: AdbClient, cx: &mut Context<Self>) -> Self {
        let (_handle, rx) = spawn_device_watcher(client.adb_path.clone());
        let this = Self {
            client,
            devices: Vec::new(),
            selected_serial: None,
            enriched: HashSet::new(),
        };
        this.spawn_bridge(rx, cx);
        this
    }

    /// Pre-populated store for tests and for the no-SDK fallback: no watcher,
    /// and the given devices are rendered exactly as provided (no enrichment).
    pub fn with_devices(client: AdbClient, devices: Vec<Device>, cx: &mut Context<Self>) -> Self {
        let enriched = devices.iter().map(|d| d.serial.clone()).collect();
        let mut this = Self {
            client,
            devices,
            selected_serial: None,
            enriched,
        };
        if this.reconcile_selection() {
            cx.emit(DeviceStoreEvent::Changed);
        }
        this
    }

    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    pub fn selected_serial(&self) -> Option<&str> {
        self.selected_serial.as_deref()
    }

    pub fn client(&self) -> &AdbClient {
        &self.client
    }

    /// Select a device. Only `device`-state rows are selectable; offline and
    /// unauthorized rows stay disabled in the UI and are ignored here.
    pub fn select(&mut self, serial: &str, cx: &mut Context<Self>) {
        let usable = self
            .devices
            .iter()
            .any(|d| d.serial == serial && d.state == DeviceState::Device);
        if usable && self.selected_serial.as_deref() != Some(serial) {
            self.selected_serial = Some(serial.to_string());
            cx.emit(DeviceStoreEvent::Changed);
            cx.notify();
        }
    }

    fn spawn_bridge(&self, rx: Receiver<Vec<Device>>, cx: &mut Context<Self>) {
        let rx = Arc::new(Mutex::new(Some(rx)));
        cx.spawn(
            async move |this: WeakEntity<DeviceStore>, cx: &mut AsyncApp| {
                loop {
                    let msg = cx
                        .background_spawn({
                            let rx = rx.clone();
                            async move {
                                let mut guard = rx.lock().unwrap();
                                match guard.as_mut() {
                                    Some(rx) => match rx.recv_timeout(Duration::from_millis(100)) {
                                        Ok(snapshot) => BridgeMsg::Snapshot(snapshot),
                                        Err(RecvTimeoutError::Timeout) => BridgeMsg::Idle,
                                        Err(RecvTimeoutError::Disconnected) => {
                                            BridgeMsg::Disconnected
                                        }
                                    },
                                    None => BridgeMsg::Disconnected,
                                }
                            }
                        })
                        .await;

                    match msg {
                        BridgeMsg::Snapshot(snapshot) => {
                            this.update(cx, |store, cx| store.apply_snapshot(snapshot, cx))
                                .ok();
                        }
                        BridgeMsg::Idle => {
                            // The store entity was released: end the task so the
                            // receiver drops and the watcher kills adb.
                            if this.upgrade().is_none() {
                                break;
                            }
                        }
                        BridgeMsg::Disconnected => break,
                    }
                }
            },
        )
        .detach();
    }

    /// Fold one watcher snapshot into the device list: carry enriched models
    /// across snapshots, keep the selection on a usable device (auto-selecting
    /// the first `device`-state row, or dropping the selection back to none),
    /// and enqueue model enrichment for newly seen devices.
    fn apply_snapshot(&mut self, snapshot: Vec<Device>, cx: &mut Context<Self>) {
        let mut merged = Vec::with_capacity(snapshot.len());
        for mut device in snapshot {
            // track-devices rows carry no model; keep one we already fetched.
            if let Some(existing) = self.devices.iter().find(|d| d.serial == device.serial) {
                device.model = existing.model.clone().or(device.model);
            }
            merged.push(device);
        }

        let changed = merged != self.devices;
        prune_enriched(&mut self.enriched, &merged);
        self.devices = merged;
        let selection_changed = self.reconcile_selection();
        self.enqueue_enrichment(cx);

        if changed || selection_changed {
            cx.emit(DeviceStoreEvent::Changed);
            cx.notify();
        }
    }

    /// Keep the selection valid: only `device`-state rows are eligible; when
    /// the current selection leaves that state (or the list changes), fall
    /// back to the first usable device, or to none (the empty state).
    fn reconcile_selection(&mut self) -> bool {
        let usable = |d: &Device| d.state == DeviceState::Device;
        let selection_valid = self.selected_serial.as_ref().is_some_and(|serial| {
            self.devices
                .iter()
                .any(|d| &d.serial == serial && usable(d))
        });
        if selection_valid {
            return false;
        }
        let fallback = self
            .devices
            .iter()
            .find(|d| usable(d))
            .map(|d| d.serial.clone());
        self.selected_serial = fallback;
        true
    }

    /// Fetch `ro.product.model` once per newly seen device-state device, on a
    /// background thread. Failures just leave the model empty; the picker
    /// label falls back to the serial.
    fn enqueue_enrichment(&mut self, cx: &mut Context<Self>) {
        let serials: Vec<String> = self
            .devices
            .iter()
            .filter(|d| d.state == DeviceState::Device && d.model.is_none())
            .map(|d| d.serial.clone())
            .collect();

        for serial in serials {
            if !self.enriched.insert(serial.clone()) {
                continue;
            }
            let client = self.client.clone();
            let weak = cx.entity().downgrade();
            let serial_for_task = serial.clone();
            cx.spawn(async move |_this, cx: &mut AsyncApp| {
                let model = cx
                    .background_spawn(async move {
                        client
                            .shell(&serial_for_task, &["getprop", "ro.product.model"])
                            .map(|output| output.trim().to_string())
                            .ok()
                    })
                    .await;
                if let Some(model) = model.filter(|model| !model.is_empty()) {
                    weak.update(cx, |store, cx| {
                        if let Some(device) = store.devices.iter_mut().find(|d| d.serial == serial)
                            && device.model.is_none()
                        {
                            device.model = Some(model);
                            cx.emit(DeviceStoreEvent::Changed);
                            cx.notify();
                        }
                    })
                    .ok();
                }
            })
            .detach();
        }
    }
}

impl EventEmitter<DeviceStoreEvent> for DeviceStore {}

/// Forget serials no longer in the snapshot so a device that disconnects and
/// reconnects is re-enriched when it comes back.
fn prune_enriched(enriched: &mut HashSet<String>, devices: &[Device]) {
    enriched.retain(|serial| devices.iter().any(|d| d.serial == *serial));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(serial: &str) -> Device {
        Device {
            serial: serial.to_string(),
            state: DeviceState::Device,
            model: None,
        }
    }

    #[test]
    fn prune_forgets_departed_serials_and_keeps_present_ones() {
        let mut enriched: HashSet<String> = ["pixel-8".to_string(), "pixel-9".to_string()].into();
        let devices = [device("pixel-9"), device("emulator-5554")];

        prune_enriched(&mut enriched, &devices);

        assert!(!enriched.contains("pixel-8"));
        assert!(enriched.contains("pixel-9"));
        assert!(!enriched.contains("emulator-5554"));
    }

    #[test]
    fn pruned_serial_is_re_enriched_after_reconnect() {
        // A device was enriched, left the snapshot, and came back: the pruned
        // serial must not block a fresh enrichment round.
        let mut enriched: HashSet<String> = ["pixel-8".to_string()].into();
        prune_enriched(&mut enriched, &[]);
        assert!(enriched.is_empty());

        let devices = [device("pixel-8")];
        prune_enriched(&mut enriched, &devices);
        assert!(enriched.is_empty());

        // enqueue_enrichment's insert() now succeeds again for the reconnected
        // serial.
        assert!(enriched.insert("pixel-8".to_string()));
    }
}
