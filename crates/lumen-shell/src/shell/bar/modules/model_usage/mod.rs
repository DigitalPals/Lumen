mod factory;
mod messages;
mod watchers;

use std::{rc::Rc, sync::Arc};

use gtk::prelude::*;
use lumen_config::{ClickAction, ConfigProperty, ConfigService, schemas::styling::CssToken};
use lumen_model_usage::ModelUsageService;
use lumen_widgets::prelude::{
    BarButton, BarButtonBehavior, BarButtonColors, BarButtonInit, BarButtonInput, BarButtonOutput,
};
use relm4::prelude::*;

pub(crate) use self::{
    factory::Factory,
    messages::{ModelUsageCmd, ModelUsageInit, ModelUsageMsg},
};
use crate::shell::bar::dropdowns::{self, DropdownRegistry};

pub(crate) struct ModelUsageModule {
    bar_button: Controller<BarButton>,
    config: Arc<ConfigService>,
    dropdowns: Rc<DropdownRegistry>,
    model_usage: Arc<ModelUsageService>,
    dynamic_class: Option<&'static str>,
}

#[relm4::component(pub(crate))]
impl Component for ModelUsageModule {
    type Init = ModelUsageInit;
    type Input = ModelUsageMsg;
    type Output = ();
    type CommandOutput = ModelUsageCmd;

    view! {
        gtk::Box {
            add_css_class: "model-usage",

            #[local_ref]
            bar_button -> gtk::MenuButton {},
        }
    }

    fn init(
        init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model_usage_config = init.config.config().modules.model_usage.clone();

        let bar_button = BarButton::builder()
            .launch(BarButtonInit {
                icon: model_usage_config.icon_name.get(),
                label: String::from("Models --"),
                tooltip: None,
                colors: BarButtonColors {
                    icon_color: model_usage_config.icon_color.clone(),
                    label_color: model_usage_config.label_color.clone(),
                    icon_background: model_usage_config.icon_bg_color.clone(),
                    button_background: model_usage_config.button_bg_color.clone(),
                    border_color: model_usage_config.border_color.clone(),
                    auto_icon_color: CssToken::Accent,
                },
                behavior: BarButtonBehavior {
                    label_max_chars: model_usage_config.label_max_length.clone(),
                    show_icon: model_usage_config.icon_show.clone(),
                    show_label: model_usage_config.label_show.clone(),
                    show_border: model_usage_config.border_show.clone(),
                    visible: ConfigProperty::new(true),
                },
                settings: init.settings,
            })
            .forward(sender.input_sender(), |output| match output {
                BarButtonOutput::LeftClick => ModelUsageMsg::LeftClick,
                BarButtonOutput::RightClick => ModelUsageMsg::RightClick,
                BarButtonOutput::MiddleClick => ModelUsageMsg::MiddleClick,
                BarButtonOutput::ScrollUp => ModelUsageMsg::ScrollUp,
                BarButtonOutput::ScrollDown => ModelUsageMsg::ScrollDown,
            });

        watchers::spawn_watchers(&sender, &model_usage_config, &init.model_usage);

        let model = Self {
            bar_button,
            config: init.config,
            dropdowns: init.dropdowns,
            model_usage: init.model_usage,
            dynamic_class: None,
        };
        let bar_button = model.bar_button.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        let model_usage = &self.config.config().modules.model_usage;

        let action = match msg {
            ModelUsageMsg::LeftClick => model_usage.left_click.get(),
            ModelUsageMsg::RightClick => model_usage.right_click.get(),
            ModelUsageMsg::MiddleClick => model_usage.middle_click.get(),
            ModelUsageMsg::ScrollUp => model_usage.scroll_up.get(),
            ModelUsageMsg::ScrollDown => model_usage.scroll_down.get(),
        };

        // Opening the dropdown is the moment the user looks at the numbers;
        // refresh so they never act on minutes-old data.
        if matches!(action, ClickAction::Dropdown(_)) {
            self.model_usage.refresh();
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
            ModelUsageCmd::Update { label, class } => {
                self.bar_button.emit(BarButtonInput::SetLabel(label));

                if let Some(old_class) = self.dynamic_class.take() {
                    root.remove_css_class(old_class);
                }
                if let Some(class) = class {
                    root.add_css_class(class);
                    self.dynamic_class = Some(class);
                }
            }
        }
    }
}
