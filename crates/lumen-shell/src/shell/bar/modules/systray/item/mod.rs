mod helpers;
mod methods;
mod watchers;

use std::sync::Arc;

use gtk4::{EventControllerScroll, EventControllerScrollFlags, gio::SimpleActionGroup};
use lumen_config::ConfigService;
use lumen_systray::{core::item::TrayItem, error::Error, types::Coordinates};
use relm4::{
    gtk::{self, prelude::*},
    prelude::*,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

pub(super) struct SystrayItemInit {
    pub(super) item: Arc<TrayItem>,
    pub(super) config: Arc<ConfigService>,
}

pub(super) struct SystrayItem {
    item: Arc<TrayItem>,
    config: Arc<ConfigService>,
    button: Option<gtk::Button>,
    icon: Option<gtk::Image>,
    icon_color_provider: Option<gtk::CssProvider>,
    popover: Option<gtk::PopoverMenu>,
    action_group: Option<SimpleActionGroup>,
    registered_accels: Vec<String>,
    cancel_token: CancellationToken,
}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub(super) enum SystrayItemMsg {
    LeftClick(Coordinates),
    RightClick(Coordinates),
    MiddleClick(Coordinates),
    Scroll {
        delta: i32,
        orientation: &'static str,
    },
    ShowMenu(Coordinates),
    MenuUpdated,
    IconUpdated,
}

#[derive(Debug)]
pub(super) enum SystrayItemOutput {}

#[relm4::factory(pub(super))]
impl FactoryComponent for SystrayItem {
    type Init = SystrayItemInit;
    type Input = SystrayItemMsg;
    type Output = SystrayItemOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::Box;

    view! {
        #[root]
        gtk::Button {
            set_css_classes: &["systray-item"],
            set_cursor_from_name: Some("pointer"),

            #[name = "icon"]
            gtk::Image {},
        }
    }

    fn init_model(
        init: Self::Init,
        _index: &relm4::factory::DynamicIndex,
        _sender: relm4::prelude::FactorySender<Self>,
    ) -> Self {
        Self {
            item: init.item,
            config: init.config,
            button: None,
            icon: None,
            icon_color_provider: None,
            popover: None,
            action_group: None,
            registered_accels: Vec::new(),
            cancel_token: CancellationToken::new(),
        }
    }

    fn init_widgets(
        &mut self,
        _index: &relm4::factory::DynamicIndex,
        root: Self::Root,
        _returned_widget: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
        sender: relm4::prelude::FactorySender<Self>,
    ) -> Self::Widgets {
        let item_id = self.item.id.get();
        root.set_widget_name(&item_id);
        debug!(item_id = %item_id, "init_widgets: setting up button");

        self.button = Some(root.clone());

        let click = gtk::GestureClick::builder().button(0).build();
        click.connect_released({
            let sender = sender.clone();
            move |gesture, _, x, y| {
                let coords = Coordinates::new(x.round() as i32, y.round() as i32);
                let msg = match gesture.current_button() {
                    1 => SystrayItemMsg::LeftClick(coords),
                    2 => SystrayItemMsg::MiddleClick(coords),
                    3 => SystrayItemMsg::RightClick(coords),
                    _ => return,
                };

                gesture.set_state(gtk::EventSequenceState::Claimed);
                sender.input(msg);
            }
        });

        let scroll = EventControllerScroll::new(
            EventControllerScrollFlags::VERTICAL | EventControllerScrollFlags::HORIZONTAL,
        );
        scroll.connect_scroll({
            let sender = sender.clone();
            move |controller, dx, dy| {
                let horizontal = dx.abs() > dy.abs();
                let amount = if horizontal { dx } else { dy };

                if amount == 0.0 {
                    return gtk::glib::Propagation::Proceed;
                }

                let delta = (amount * 120.0).round() as i32;
                if delta == 0 {
                    return gtk::glib::Propagation::Proceed;
                }

                let orientation = if horizontal { "horizontal" } else { "vertical" };
                debug!(
                    dx,
                    dy,
                    delta,
                    orientation,
                    item_id = %item_id,
                    classes = ?controller.widget().map(|widget| widget.css_classes()),
                    "systray scroll"
                );
                sender.input(SystrayItemMsg::Scroll { delta, orientation });
                gtk::glib::Propagation::Stop
            }
        });

        root.add_controller(click);
        root.add_controller(scroll);

        watchers::spawn_menu_watcher(&sender, &self.item, self.cancel_token.clone());
        watchers::spawn_icon_watcher(&sender, &self.item, self.cancel_token.clone());

        let widgets = view_output!();

        self.icon = Some(widgets.icon.clone());
        self.update_icon(&widgets.icon);

        widgets
    }

    fn update(&mut self, msg: Self::Input, _sender: relm4::prelude::FactorySender<Self>) {
        match msg {
            SystrayItemMsg::LeftClick(coords) => {
                let item = self.item.clone();
                let sender = _sender.clone();
                let item_is_menu = item.item_is_menu.get();
                if item_is_menu {
                    self.request_menu_show(&sender, coords);
                    return;
                }

                tokio::spawn(async move {
                    let result = item.activate(coords).await;
                    match result {
                        Ok(()) => {}
                        Err(Error::OperationNotSupported { .. }) => {
                            debug!(
                                id = %item.id.get(),
                                bus_name = %item.bus_name.get(),
                                "systray activate unsupported, showing menu instead"
                            );
                            if let Err(error) = item.refresh_menu().await {
                                debug!(error = %error, "AboutToShow not supported");
                            }
                            sender.input(SystrayItemMsg::ShowMenu(coords));
                        }
                        Err(error) => {
                            warn!(
                                id = %item.id.get(),
                                bus_name = %item.bus_name.get(),
                                error = %error,
                                "systray activate failed"
                            );
                        }
                    }
                });
            }
            SystrayItemMsg::RightClick(coords) => {
                self.request_menu_show(&_sender, coords);
            }

            SystrayItemMsg::ShowMenu(coords) => {
                self.toggle_menu(coords);
            }
            SystrayItemMsg::MiddleClick(coords) => {
                let item = self.item.clone();
                tokio::spawn(async move {
                    if let Err(error) = item.secondary_activate(coords).await {
                        warn!(
                            id = %item.id.get(),
                            bus_name = %item.bus_name.get(),
                            error = %error,
                            "systray secondary_activate failed"
                        );
                    }
                });
            }
            SystrayItemMsg::Scroll { delta, orientation } => {
                let item = self.item.clone();
                tokio::spawn(async move {
                    if let Err(error) = item.scroll(delta, orientation).await {
                        warn!(
                            id = %item.id.get(),
                            bus_name = %item.bus_name.get(),
                            delta,
                            orientation,
                            error = %error,
                            "systray scroll failed"
                        );
                    }
                });
            }
            SystrayItemMsg::MenuUpdated => {
                self.rebuild_menu_if_visible();
            }
            SystrayItemMsg::IconUpdated => {
                if let Some(icon) = self.icon.clone() {
                    self.update_icon(&icon);
                }
            }
        }
    }
}

impl Drop for SystrayItem {
    fn drop(&mut self) {
        self.cancel_token.cancel();
        self.clear_accelerators();
        if let Some(popover) = self.popover.take() {
            popover.unparent();
        }
    }
}
