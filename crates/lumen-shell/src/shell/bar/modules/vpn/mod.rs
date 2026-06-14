mod factory;
mod helpers;
mod messages;
mod methods;
mod watchers;

use std::{rc::Rc, sync::Arc};

use gtk::prelude::*;
use lumen_config::{ConfigProperty, ConfigService, schemas::styling::CssToken};
use lumen_network::NetworkService;
use lumen_widgets::prelude::{
    BarButton, BarButtonBehavior, BarButtonColors, BarButtonInit, BarButtonInput, BarButtonOutput,
};
use relm4::prelude::*;

pub(crate) use self::{
    factory::Factory,
    messages::{VpnCmd, VpnInit, VpnMsg},
};
use crate::shell::bar::dropdowns::{self, DropdownRegistry};

pub(crate) struct VpnModule {
    bar_button: Controller<BarButton>,
    config: Arc<ConfigService>,
    network: Arc<NetworkService>,
    dropdowns: Rc<DropdownRegistry>,
}

#[relm4::component(pub(crate))]
impl Component for VpnModule {
    type Init = VpnInit;
    type Input = VpnMsg;
    type Output = ();
    type CommandOutput = VpnCmd;

    view! {
        gtk::Box {
            add_css_class: "vpn",

            #[local_ref]
            bar_button -> gtk::MenuButton {},
        }
    }

    fn init(
        init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let config = init.config.config();
        let vpn_config = &config.modules.vpn;
        let (initial_icon, initial_label) = Self::compute_display(vpn_config, &init.network);

        let bar_button = BarButton::builder()
            .launch(BarButtonInit {
                icon: initial_icon,
                label: initial_label,
                tooltip: None,
                colors: BarButtonColors {
                    icon_color: vpn_config.icon_color.clone(),
                    label_color: vpn_config.label_color.clone(),
                    icon_background: vpn_config.icon_bg_color.clone(),
                    button_background: vpn_config.button_bg_color.clone(),
                    border_color: vpn_config.border_color.clone(),
                    auto_icon_color: CssToken::Accent,
                },
                behavior: BarButtonBehavior {
                    label_max_chars: vpn_config.label_max_length.clone(),
                    show_icon: vpn_config.icon_show.clone(),
                    show_label: vpn_config.label_show.clone(),
                    show_border: vpn_config.border_show.clone(),
                    visible: ConfigProperty::new(true),
                },
                settings: init.settings,
            })
            .forward(sender.input_sender(), |output| match output {
                BarButtonOutput::LeftClick => VpnMsg::LeftClick,
                BarButtonOutput::RightClick => VpnMsg::RightClick,
                BarButtonOutput::MiddleClick => VpnMsg::MiddleClick,
                BarButtonOutput::ScrollUp => VpnMsg::ScrollUp,
                BarButtonOutput::ScrollDown => VpnMsg::ScrollDown,
            });

        watchers::spawn(&sender, vpn_config, &init.network);

        let model = Self {
            bar_button,
            config: init.config,
            network: init.network,
            dropdowns: init.dropdowns,
        };
        let bar_button = model.bar_button.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        let config = &self.config.config().modules.vpn;

        let action = match msg {
            VpnMsg::LeftClick => config.left_click.get(),
            VpnMsg::RightClick => config.right_click.get(),
            VpnMsg::MiddleClick => config.middle_click.get(),
            VpnMsg::ScrollUp => config.scroll_up.get(),
            VpnMsg::ScrollDown => config.scroll_down.get(),
        };

        dropdowns::dispatch_click(&action, &self.dropdowns, &self.bar_button);
    }

    fn update_cmd(&mut self, msg: VpnCmd, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            VpnCmd::StateChanged => {
                let config = &self.config.config().modules.vpn;
                let (icon, label) = Self::compute_display(config, &self.network);
                self.bar_button.emit(BarButtonInput::SetIcon(icon));
                self.bar_button.emit(BarButtonInput::SetLabel(label));
            }
        }
    }
}
