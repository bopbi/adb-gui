use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme, Disableable, IndexPath, StyledExt as _, TitleBar, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, TestSupportExt, Window, div, rems,
};

use adb_core::{AdbClient, Device, DeviceState, find_adb};

use crate::device_store::DeviceStore;
use crate::executor::{AdbExecutor, FeatureExecutor};
use crate::feature::{FeatureInput, FeatureKind, FeatureRegistry, FeatureState, all_features};

/// A device as shown in the picker: "model · serial", with the state appended
/// for rows that are not usable. Unusable rows render disabled.
#[derive(Clone, PartialEq)]
struct DevicePickerItem {
    label: SharedString,
    serial: String,
    usable: bool,
}

impl DevicePickerItem {
    fn from_device(device: &Device) -> Self {
        let state = device.state.to_string();
        let label = match (&device.model, device.state) {
            (Some(model), DeviceState::Device) => format!("{model} · {}", device.serial),
            (Some(model), _) => format!("{model} · {} ({state})", device.serial),
            (None, DeviceState::Device) => device.serial.clone(),
            (None, _) => format!("{} ({state})", device.serial),
        };
        Self {
            label: label.into(),
            serial: device.serial.clone(),
            usable: device.state == DeviceState::Device,
        }
    }
}

impl SearchableListItem for DevicePickerItem {
    type Value = String;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.serial
    }

    fn disabled(&self) -> bool {
        !self.usable
    }
}

/// Live state of one feature cell in the grid.
struct FeatureCell {
    state: FeatureState,
    busy: bool,
    error: Option<String>,
}

impl FeatureCell {
    fn new(kind: &FeatureKind) -> Self {
        let state = match kind {
            FeatureKind::Toggle => FeatureState::Toggle(false),
            _ => FeatureState::None,
        };
        Self {
            state,
            busy: false,
            error: None,
        }
    }
}

/// The composition root: device store + feature registry + executor, with the
/// window chrome and grid rendered from them.
pub struct AppView {
    client: AdbClient,
    device_store: Entity<DeviceStore>,
    device_picker: Entity<SelectState<Vec<DevicePickerItem>>>,
    registry: FeatureRegistry,
    executor: Arc<dyn FeatureExecutor>,
    cells: Vec<FeatureCell>,
    /// Snapshot of the store for rendering; refreshed on `DeviceStoreEvent`.
    devices: Vec<Device>,
    selected_serial: Option<String>,
    startup_error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        match find_adb() {
            Ok(adb_path) => {
                let client = AdbClient::new(adb_path);
                let device_store = cx.new(|cx| DeviceStore::new(client.clone(), cx));
                Self::with_parts(
                    client,
                    device_store,
                    all_features(),
                    Arc::new(AdbExecutor),
                    None,
                    window,
                    cx,
                )
            }
            Err(err) => {
                // No SDK: keep the window usable, show the reason, stay empty.
                let client = AdbClient::new(PathBuf::from("adb"));
                let device_store =
                    cx.new(|cx| DeviceStore::with_devices(client.clone(), Vec::new(), cx));
                Self::with_parts(
                    client,
                    device_store,
                    all_features(),
                    Arc::new(AdbExecutor),
                    Some(err.to_string()),
                    window,
                    cx,
                )
            }
        }
    }

    /// Injectable parts for UI tests: a pre-populated store, a stub registry
    /// and a stub executor — no adb required.
    pub fn with_parts(
        client: AdbClient,
        device_store: Entity<DeviceStore>,
        registry: FeatureRegistry,
        executor: Arc<dyn FeatureExecutor>,
        startup_error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (devices, selected_serial) = {
            let store = device_store.read(cx);
            (
                store.devices().to_vec(),
                store.selected_serial().map(str::to_string),
            )
        };
        let picker_items: Vec<DevicePickerItem> =
            devices.iter().map(DevicePickerItem::from_device).collect();
        let selected_index = selected_serial
            .as_ref()
            .and_then(|serial| picker_items.iter().position(|item| &item.serial == serial))
            .map(IndexPath::new);
        let device_picker = cx.new(|cx| SelectState::new(picker_items, selected_index, window, cx));

        let store_subscription =
            cx.subscribe_in(&device_store, window, |this, _store, _event, window, cx| {
                // The store may still be locked when this runs; sync deferred.
                this.defer_sync_devices(window, cx);
            });
        let picker_subscription = cx.subscribe_in(
            &device_picker,
            window,
            |this, _picker, event: &SelectEvent<Vec<DevicePickerItem>>, _window, cx| {
                if let SelectEvent::Confirm(Some(serial)) = event {
                    this.device_store
                        .update(cx, |store, cx| store.select(serial, cx));
                }
            },
        );

        let cells = registry
            .features()
            .iter()
            .map(|feature| FeatureCell::new(&feature.kind()))
            .collect();

        let mut this = Self {
            client,
            device_store,
            device_picker,
            registry,
            executor,
            cells,
            devices,
            selected_serial,
            startup_error,
            _subscriptions: vec![store_subscription, picker_subscription],
        };
        // The store may already hold a selection (pre-populated store, or a
        // snapshot that landed between its creation and our subscription):
        // sync_devices only refreshes on a *change* of selection, so trigger
        // the initial state read explicitly.
        if this.selected_serial.is_some() {
            this.refresh_all(window, cx);
        }
        this
    }

    fn defer_sync_devices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.defer_in(window, |this, window, cx| {
            this.sync_devices(window, cx);
        });
    }

    /// Copy the store snapshot for rendering and mirror it into the picker.
    fn sync_devices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (devices, selected_serial) = {
            let store = self.device_store.read(cx);
            (
                store.devices().to_vec(),
                store.selected_serial().map(str::to_string),
            )
        };
        let selection_became_usable =
            selected_serial.is_some() && selected_serial != self.selected_serial;

        self.devices = devices;
        self.selected_serial = selected_serial.clone();

        let items: Vec<DevicePickerItem> = self
            .devices
            .iter()
            .map(DevicePickerItem::from_device)
            .collect();
        self.device_picker.update(cx, |picker, cx| {
            picker.set_items(items, window, cx);
            if let Some(serial) = &selected_serial {
                picker.set_selected_value(serial, window, cx);
            }
        });

        if selection_became_usable {
            self.refresh_all(window, cx);
        }
        cx.notify();
    }

    /// Read every feature's initial state for the newly selected device.
    /// Errors are ignored here (the perform path surfaces them).
    fn refresh_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(serial) = self.selected_serial.clone() else {
            return;
        };
        for ix in 0..self.registry.features().len() {
            let feature = self.registry.features()[ix].clone();
            let executor = self.executor.clone();
            let client = self.client.clone();
            let serial = serial.clone();
            self.cells[ix].busy = true;
            cx.spawn_in(window, async move |this, cx| {
                let result = cx
                    .background_spawn(async move { executor.read(client, feature, serial) })
                    .await;
                this.update_in(cx, |this, _window, cx| {
                    this.cells[ix].busy = false;
                    if let Ok(state) = result {
                        this.cells[ix].state = state;
                        this.cells[ix].error = None;
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn on_feature_input(
        &mut self,
        ix: usize,
        input: FeatureInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let feature = self.registry.features()[ix].clone();
        if feature.is_destructive() {
            let weak = cx.entity().downgrade();
            let title = format!("Run “{}”?", feature.title());
            let input = Rc::new(RefCell::new(Some(input)));
            window.open_alert_dialog(cx, move |alert, _, _| {
                alert
                    .title(title.clone())
                    .description("This action cannot be undone.")
                    .confirm()
                    .ok_text("Continue")
                    .ok_variant(ButtonVariant::Danger)
                    .on_ok({
                        let weak = weak.clone();
                        let input = input.clone();
                        move |_, window, cx| {
                            if let Some(input) = input.borrow_mut().take() {
                                let _ = weak.update(cx, |view, cx| {
                                    view.dispatch(ix, input, window, cx);
                                });
                            }
                            true
                        }
                    })
            });
        } else {
            self.dispatch(ix, input, window, cx);
        }
    }

    /// Run the feature command on a background thread and refresh the cell.
    fn dispatch(
        &mut self,
        ix: usize,
        input: FeatureInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(serial) = self.selected_serial.clone() else {
            return;
        };
        let feature = self.registry.features()[ix].clone();
        let executor = self.executor.clone();
        let client = self.client.clone();
        let feature_id = feature.id();
        self.cells[ix].busy = true;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    executor.perform_and_read(client, feature, serial, input)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.cells[ix].busy = false;
                match result {
                    Ok(state) => {
                        this.cells[ix].state = state;
                        this.cells[ix].error = None;
                    }
                    Err(err) => {
                        this.cells[ix].error = Some(err.to_string());
                        window.push_notification(
                            Notification::error(format!("{feature_id}: {err}")),
                            cx,
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_device_picker(&self) -> impl IntoElement {
        h_flex().items_center().gap_2().child(
            Select::new(&self.device_picker)
                .id("device-picker")
                .placeholder("Select a device")
                .w_64()
                .disabled(self.devices.is_empty()),
        )
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(error) = &self.startup_error {
            return div()
                .id("startup-error")
                .test_support()
                .child(Alert::error("startup-error-alert", error.clone()))
                .into_any_element();
        }

        if self.devices.is_empty() {
            return div()
                .id("no-devices")
                .test_support()
                .child(
                    Alert::info(
                        "no-devices-alert",
                        "Connect a device or start an emulator — it will appear here automatically.",
                    )
                    .title("No devices"),
                )
                .into_any_element();
        }

        let grid_enabled = self.selected_serial.is_some();
        let mut body = v_flex().flex_1().min_h_0().gap_3().overflow_hidden();
        if !grid_enabled {
            body = body.child(
                div().id("no-usable-device").test_support().child(
                    Alert::warning(
                        "no-usable-device-alert",
                        "Devices are present but none are ready (offline or unauthorized).",
                    )
                    .title("No usable device"),
                ),
            );
        }
        body.child(self.render_grid(grid_enabled, cx))
            .into_any_element()
    }

    fn render_grid(&self, grid_enabled: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("feature-grid")
            .test_support()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(div().flex().flex_wrap().gap_3().children(
                (0..self.registry.features().len()).map(|ix| {
                    self.render_feature_cell(ix, grid_enabled, cx)
                        .into_any_element()
                }),
            ))
    }

    fn render_feature_cell(
        &self,
        ix: usize,
        grid_enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let feature = &self.registry.features()[ix];
        let cell = &self.cells[ix];
        let enabled = grid_enabled && !cell.busy;

        let control: AnyElement = match feature.kind() {
            FeatureKind::Toggle => {
                let checked = matches!(&cell.state, FeatureState::Toggle(true));
                Switch::new(format!("feature-toggle:{}", feature.id()))
                    .checked(checked)
                    .disabled(!enabled)
                    .on_change(cx.listener(move |this, checked: &bool, window, cx| {
                        this.on_feature_input(ix, FeatureInput::Bool(*checked), window, cx);
                    }))
                    .into_any_element()
            }
            FeatureKind::Action => Button::new(format!("feature-action:{}", feature.id()))
                .label(feature.title())
                .disabled(!enabled)
                .when(feature.is_destructive(), |button| button.danger())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.on_feature_input(ix, FeatureInput::None, window, cx);
                }))
                .into_any_element(),
            // The shell ships one Toggle; other kinds render as disabled
            // placeholders until their features land in parts 04–06.
            _ => Button::new(format!("feature-unavailable:{}", feature.id()))
                .label(feature.title())
                .disabled(true)
                .into_any_element(),
        };

        div()
            .id(format!("feature-cell:{}", feature.id()))
            .test_support()
            .w(rems(9.))
            .v_flex()
            .gap_2()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(Label::new(feature.title()))
            .child(control)
            .when_some(cell.error.clone(), |this, error| {
                this.child(Label::new(error).text_color(cx.theme().danger))
            })
            .into_any_element()
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(TitleBar::new().child(Label::new("adb-gui")))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .p_4()
                    .child(self.render_device_picker())
                    .child(self.render_body(cx)),
            )
    }
}
