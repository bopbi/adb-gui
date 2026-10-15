use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
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
    AnyElement, AppContext as _, ClipboardItem, Context, Entity, ExternalPaths, Image, ImageFormat,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TestSupportExt, Window, div, rems,
};

use adb_core::{AdbClient, Device, DeviceState, find_adb};

use crate::device_store::DeviceStore;
use crate::executor::{AdbExecutor, FeatureExecutor};
use crate::feature::{
    Availability, ClipboardPayload, Feature, FeatureInput, FeatureKind, FeatureRegistry,
    FeatureState, StepperSpec,
};
use crate::features::all_features;
use crate::features::install_apk::INSTALL_APK_ID;

/// One option of a feature's `Choice` cell, as shown in its per-cell
/// dropdown. The value is the option's index in the feature's option list.
#[derive(Clone, PartialEq)]
struct ChoiceItem {
    label: SharedString,
    index: usize,
}

impl SearchableListItem for ChoiceItem {
    type Value = usize;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.index
    }
}

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
    /// The text input backing a `FeatureKind::TextAction` cell (Enter
    /// submits its content).
    input: Option<Entity<InputState>>,
    /// The dropdown backing a `FeatureKind::Choice` cell. Selecting an
    /// option dispatches `FeatureInput::Index` — intermediate values are
    /// never applied.
    choice: Option<Entity<SelectState<Vec<ChoiceItem>>>>,
    /// Keeps the choice cell's Confirm subscription alive.
    _choice_subscription: Option<Subscription>,
}

impl FeatureCell {
    fn new(
        ix: usize,
        feature: &dyn Feature,
        window: &mut Window,
        cx: &mut Context<AppView>,
    ) -> Self {
        let kind = feature.kind();
        let state = match kind {
            FeatureKind::Toggle => FeatureState::Toggle(false),
            _ => FeatureState::None,
        };
        let input = match kind {
            FeatureKind::TextAction => Some(cx.new(|cx| {
                InputState::new(window, cx).placeholder("Text to type, then press Enter")
            })),
            _ => None,
        };
        let (choice, choice_subscription) = match kind {
            FeatureKind::Choice(options) => {
                let items: Vec<ChoiceItem> = options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| ChoiceItem {
                        label: option.label.into(),
                        index,
                    })
                    .collect();
                let choice = cx.new(|cx| SelectState::new(items, None, window, cx));
                let subscription = cx.subscribe_in(
                    &choice,
                    window,
                    move |this, _choice, event: &SelectEvent<Vec<ChoiceItem>>, window, cx| {
                        if let SelectEvent::Confirm(Some(index)) = event {
                            this.on_feature_input(ix, FeatureInput::Index(*index), window, cx);
                        }
                    },
                );
                (Some(choice), Some(subscription))
            }
            _ => (None, None),
        };
        Self {
            state,
            busy: false,
            error: None,
            input,
            choice,
            _choice_subscription: choice_subscription,
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

        let mut cells = Vec::with_capacity(registry.features().len());
        let mut subscriptions = vec![store_subscription, picker_subscription];
        for (ix, feature) in registry.features().iter().enumerate() {
            cells.push(FeatureCell::new(ix, feature.as_ref(), window, cx));
            // Enter in a text-action cell submits its content to the feature.
            if let Some(input) = cells[ix].input.clone() {
                subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    move |this, input, event: &InputEvent, window, cx| {
                        if let InputEvent::PressEnter { .. } = event {
                            let text = input.read(cx).value().to_string();
                            if !text.is_empty() {
                                this.dispatch(ix, FeatureInput::Text(text), window, cx);
                            }
                        }
                    },
                ));
            }
        }

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
            _subscriptions: subscriptions,
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

    /// Read every feature's state for the selected device in ONE batched
    /// device round trip. Errors are ignored here (the perform path surfaces
    /// them).
    fn refresh_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(serial) = self.selected_serial.clone() else {
            return;
        };
        let features: Vec<_> = self.registry.features().to_vec();
        let executor = self.executor.clone();
        let client = self.client.clone();
        for cell in &mut self.cells {
            cell.busy = true;
        }
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { executor.read_all(client, features, serial) })
                .await;
            this.update_in(cx, |this, window, cx| {
                for cell in &mut this.cells {
                    cell.busy = false;
                }
                if let Ok(states) = result {
                    for (cell, state) in this.cells.iter_mut().zip(states) {
                        cell.state = state;
                        cell.error = None;
                    }
                    this.sync_choice_selections(window, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Mirror each choice cell's batched state into its dropdown so the
    /// select shows the device's current option.
    fn sync_choice_selections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for ix in 0..self.cells.len() {
            let (Some(choice), FeatureKind::Choice(options)) = (
                self.cells[ix].choice.clone(),
                &self.registry.features()[ix].kind(),
            ) else {
                continue;
            };
            let selected = match &self.cells[ix].state {
                FeatureState::Value(tag) => options.iter().position(|opt| opt.id == *tag),
                _ => None,
            };
            choice.update(cx, |state, cx| match selected {
                Some(row) => state.set_selected_value(&row, window, cx),
                None => state.set_selected_index(None, window, cx),
            });
        }
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

    /// Run the feature command on a background thread, then re-read every
    /// feature's state in one batched round trip so all cells refresh.
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
        // Kept for the post-perform clipboard pickup (screenshots).
        let clipboard_feature = feature.clone();
        let features: Vec<_> = self.registry.features().to_vec();
        let executor = self.executor.clone();
        let client = self.client.clone();
        let feature_id = feature.id();
        self.cells[ix].busy = true;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn({
                    let executor = executor.clone();
                    let client = client.clone();
                    let serial = serial.clone();
                    async move { executor.perform(client, feature, serial, input) }
                })
                .await;
            // Re-read batched state after a successful perform so every cell
            // reflects the device; a failed perform keeps the old state.
            // The perform itself succeeded or not regardless of this re-read.
            let performed = result.is_ok();
            let states: Result<Vec<FeatureState>, adb_core::AdbError> = match result {
                Ok(()) => {
                    cx.background_spawn(async move { executor.read_all(client, features, serial) })
                        .await
                }
                Err(err) => Err(err),
            };
            this.update_in(cx, |this, window, cx| {
                this.cells[ix].busy = false;
                // Post-perform offers depend only on the perform succeeding:
                // a screenshot must reach the clipboard even if the re-read
                // fails, and a fallback-path note must surface either way.
                if performed {
                    if let Some(payload) = clipboard_feature.clipboard_payload() {
                        Self::offer_clipboard(payload, cx);
                    }
                    if let Some(notice) = clipboard_feature.notice() {
                        window.push_notification(Notification::info(notice), cx);
                    }
                }
                match states {
                    Ok(states) => {
                        for (cell, state) in this.cells.iter_mut().zip(states) {
                            cell.state = state;
                            cell.error = None;
                        }
                        this.sync_choice_selections(window, cx);
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

    /// Place a feature's post-perform payload (a screenshot PNG) on the
    /// system clipboard.
    fn offer_clipboard(payload: ClipboardPayload, cx: &mut gpui_kit::App) {
        if payload.format == "png" {
            static IMAGE_ID: AtomicU64 = AtomicU64::new(1);
            let image = Image {
                format: ImageFormat::Png,
                bytes: payload.bytes,
                id: IMAGE_ID.fetch_add(1, Ordering::Relaxed),
            };
            cx.write_to_clipboard(ClipboardItem::new_image(&image));
        }
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
        let mut grid = div()
            .id("feature-grid")
            .test_support()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .v_flex()
            .gap_4();
        for section in self.registry.sections() {
            if section.len == 0 {
                continue;
            }
            let mut group = v_flex().gap_2();
            if !section.title.is_empty() {
                group =
                    group.child(Label::new(section.title).text_color(cx.theme().muted_foreground));
            }
            grid = grid.child(group.child(div().flex().flex_wrap().gap_3().children(
                (section.start..section.start + section.len).map(|ix| {
                    self.render_feature_cell(ix, grid_enabled, cx)
                        .into_any_element()
                }),
            )));
        }
        grid
    }

    fn render_feature_cell(
        &self,
        ix: usize,
        grid_enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let feature = &self.registry.features()[ix];
        let cell = &self.cells[ix];
        // The emulator-only guard lives here, once: features never re-check.
        let selected_is_emulator = self
            .selected_serial
            .as_ref()
            .and_then(|serial| self.devices.iter().find(|d| &d.serial == serial))
            .is_some_and(|device| device.is_emulator());
        let emulator_only_blocked =
            matches!(feature.availability(), Availability::EmulatorOnly) && !selected_is_emulator;
        let enabled = grid_enabled && !cell.busy && !emulator_only_blocked;
        let blocked_reason: Option<&str> =
            emulator_only_blocked.then_some("Emulator only — connect an emulator to use this");

        let control: AnyElement = match feature.kind() {
            FeatureKind::Toggle => {
                let checked = matches!(&cell.state, FeatureState::Toggle(true));
                let switch = Switch::new(format!("feature-toggle:{}", feature.id()))
                    .checked(checked)
                    .disabled(!enabled)
                    .on_change(cx.listener(move |this, checked: &bool, window, cx| {
                        this.on_feature_input(ix, FeatureInput::Bool(*checked), window, cx);
                    }));
                // Tooltips are supported on Switch and Button; other control
                // kinds show the reason as a cell label below.
                let switch = if let Some(reason) = blocked_reason {
                    switch.tooltip(reason)
                } else {
                    switch
                };
                switch.into_any_element()
            }
            FeatureKind::Stepper(spec) => {
                let current = match &cell.state {
                    FeatureState::Value(value) => Some(value.as_str()),
                    _ => None,
                };
                let (minus_input, plus_input) = spec.step_inputs(current);
                // Four controls do not fit one 144px cell row; the stepper is
                // two rows: − value +, then the optional reset.
                let mut control = v_flex().gap_1().child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new(format!("feature-stepper-minus:{}", feature.id()))
                                .label("−")
                                .compact()
                                .disabled(!enabled)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.on_feature_input(ix, minus_input.clone(), window, cx);
                                })),
                        )
                        .child(
                            div()
                                .id(format!("feature-stepper-value:{}", feature.id()))
                                .test_support()
                                .flex_1()
                                .justify_center()
                                .child(Label::new(current.unwrap_or("Default"))),
                        )
                        .child(
                            Button::new(format!("feature-stepper-plus:{}", feature.id()))
                                .label("+")
                                .compact()
                                .disabled(!enabled)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.on_feature_input(ix, plus_input.clone(), window, cx);
                                })),
                        ),
                );
                if let StepperSpec::Relative {
                    supports_reset: true,
                } = spec
                {
                    control = control.child(
                        h_flex().justify_end().child(
                            Button::new(format!("feature-stepper-reset:{}", feature.id()))
                                .label("Reset")
                                .compact()
                                .disabled(!enabled)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.on_feature_input(ix, FeatureInput::Reset, window, cx);
                                })),
                        ),
                    );
                }
                control.into_any_element()
            }
            FeatureKind::Choice(_) => {
                // A real dropdown: selecting an option applies exactly that
                // option — cycling through intermediates is gone (G3).
                let choice = cell
                    .choice
                    .as_ref()
                    .expect("choice cells are created with a select");
                Select::new(choice)
                    .id(format!("feature-choice:{}", feature.id()))
                    .placeholder("Select…")
                    .w_full()
                    .disabled(!enabled)
                    .into_any_element()
            }
            FeatureKind::Action => {
                let button = Button::new(format!("feature-action:{}", feature.id()))
                    .label(feature.title())
                    .disabled(!enabled)
                    .when(feature.is_destructive(), |button| button.danger())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.on_feature_input(ix, FeatureInput::None, window, cx);
                    }));
                let button = if let Some(reason) = blocked_reason {
                    button.tooltip(reason)
                } else {
                    button
                };
                button.into_any_element()
            }
            FeatureKind::TextAction => {
                let input = cell
                    .input
                    .as_ref()
                    .expect("text-action cells are created with an input");
                Input::new(input)
                    .id(format!("feature-input:{}", feature.id()))
                    .disabled(!enabled)
                    .into_any_element()
            }
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
            .when_some(blocked_reason, |this, reason| {
                this.child(Label::new(reason).text_color(cx.theme().muted_foreground))
            })
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
            // Dropping an .apk anywhere on the window installs it.
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                let Some(ix) = this
                    .registry
                    .features()
                    .iter()
                    .position(|feature| feature.id() == INSTALL_APK_ID)
                else {
                    return;
                };
                for path in paths.paths() {
                    let text = path.to_string_lossy().into_owned();
                    if text.to_ascii_lowercase().ends_with(".apk") {
                        this.dispatch(ix, FeatureInput::Text(text), window, cx);
                    }
                }
            }))
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
