mod factory;
mod messages;
mod watchers;

use std::{rc::Rc, sync::Arc};

use gtk::prelude::*;
use lumen_config::{ClickAction, ConfigProperty, ConfigService, schemas::styling::CssToken};
use lumen_hermes::HermesChatService;
use lumen_widgets::prelude::{
    BarButton, BarButtonBehavior, BarButtonColors, BarButtonInit, BarButtonInput, BarButtonOutput,
};
use relm4::prelude::*;

pub(crate) use self::{
    factory::Factory,
    messages::{HermesChatCmd, HermesChatInit, HermesChatMsg},
};
use crate::{
    bootstrap::hermes_chat,
    shell::bar::dropdowns::{self, DropdownRegistry},
};

pub(crate) struct HermesChatModule {
    bar_button: Controller<BarButton>,
    config: Arc<ConfigService>,
    dropdowns: Rc<DropdownRegistry>,
    hermes_chat: Arc<HermesChatService>,
    dynamic_class: Option<&'static str>,
}

#[relm4::component(pub(crate))]
impl Component for HermesChatModule {
    type Init = HermesChatInit;
    type Input = HermesChatMsg;
    type Output = ();
    type CommandOutput = HermesChatCmd;

    view! {
        gtk::Box {
            add_css_class: "hermes-chat",

            #[local_ref]
            bar_button -> gtk::MenuButton {},
        }
    }

    fn init(
        init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let hermes_config = init.config.config().modules.hermes_chat.clone();

        let bar_button = BarButton::builder()
            .launch(BarButtonInit {
                icon: hermes_config.icon_name.get(),
                label: String::from("Hermes"),
                tooltip: Some(String::from("Chat with Hermes Agent")),
                colors: BarButtonColors {
                    icon_color: hermes_config.icon_color.clone(),
                    label_color: hermes_config.label_color.clone(),
                    icon_background: hermes_config.icon_bg_color.clone(),
                    button_background: hermes_config.button_bg_color.clone(),
                    border_color: hermes_config.border_color.clone(),
                    auto_icon_color: CssToken::Accent,
                },
                behavior: BarButtonBehavior {
                    label_max_chars: hermes_config.label_max_length.clone(),
                    show_icon: hermes_config.icon_show.clone(),
                    show_label: hermes_config.label_show.clone(),
                    show_border: hermes_config.border_show.clone(),
                    visible: ConfigProperty::new(true),
                },
                settings: init.settings,
            })
            .forward(sender.input_sender(), |output| match output {
                BarButtonOutput::LeftClick => HermesChatMsg::LeftClick,
                BarButtonOutput::RightClick => HermesChatMsg::RightClick,
                BarButtonOutput::MiddleClick => HermesChatMsg::MiddleClick,
                BarButtonOutput::ScrollUp => HermesChatMsg::ScrollUp,
                BarButtonOutput::ScrollDown => HermesChatMsg::ScrollDown,
            });

        watchers::spawn_watchers(&sender, &hermes_config, &init.hermes_chat);

        let model = Self {
            bar_button,
            config: init.config,
            dropdowns: init.dropdowns,
            hermes_chat: init.hermes_chat,
            dynamic_class: None,
        };
        let bar_button = model.bar_button.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        let config = self.config.config();
        let hermes = &config.modules.hermes_chat;
        let action = match msg {
            HermesChatMsg::LeftClick => hermes.left_click.get(),
            HermesChatMsg::RightClick => hermes.right_click.get(),
            HermesChatMsg::MiddleClick => hermes.middle_click.get(),
            HermesChatMsg::ScrollUp => hermes.scroll_up.get(),
            HermesChatMsg::ScrollDown => hermes.scroll_down.get(),
        };

        if matches!(action, ClickAction::Dropdown(_)) {
            self.hermes_chat.connect();
        }

        dropdowns::dispatch_click(&action, &self.dropdowns, &self.bar_button);
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        _sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        match msg {
            HermesChatCmd::Update { label, class } => {
                self.bar_button.emit(BarButtonInput::SetLabel(label));
                if let Some(old_class) = self.dynamic_class.take() {
                    root.remove_css_class(old_class);
                }
                if let Some(class) = class {
                    root.add_css_class(class);
                    self.dynamic_class = Some(class);
                }
            }
            HermesChatCmd::ConfigChanged => {
                let config = hermes_chat::connection_config(&self.config.config().modules);
                self.hermes_chat.update_config(config);
            }
        }
    }
}
