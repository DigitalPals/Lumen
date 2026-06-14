mod factory;
mod messages;
mod methods;
mod row;
mod watchers;

use std::sync::Arc;

use gtk::prelude::*;
use lumen_network::NetworkService;
use lumen_widgets::prelude::*;
use relm4::{factory::FactoryVecDeque, gtk, prelude::*};

pub(super) use self::factory::Factory;
use self::{
    messages::{VpnDropdownCmd, VpnDropdownInit, VpnDropdownMsg},
    row::VpnRow,
};
use crate::{i18n::t, shell::bar::dropdowns::scaled_dimension};

const BASE_WIDTH: f32 = 382.0;
const BASE_HEIGHT: f32 = 512.0;

pub(crate) struct VpnDropdown {
    network: Arc<NetworkService>,
    scaled_width: i32,
    scaled_height: i32,
    active_list: FactoryVecDeque<VpnRow>,
    profile_list: FactoryVecDeque<VpnRow>,
    has_active: bool,
    has_profiles: bool,
    operation_error: Option<String>,
}

#[relm4::component(pub(crate))]
impl Component for VpnDropdown {
    type Init = VpnDropdownInit;
    type Input = VpnDropdownMsg;
    type Output = ();
    type CommandOutput = VpnDropdownCmd;

    view! {
        #[root]
        gtk::Popover {
            set_css_classes: &["dropdown", "network-dropdown", "vpn-dropdown"],
            set_has_arrow: false,
            #[watch]
            set_width_request: model.scaled_width,
            #[watch]
            set_height_request: model.scaled_height,

            #[template]
            Dropdown {
                set_overflow: gtk::Overflow::Hidden,

                #[template]
                DropdownHeader {
                    #[template_child]
                    icon {
                        set_visible: true,
                        set_icon_name: Some("ld-shield-symbolic"),
                    },
                    #[template_child]
                    label {
                        set_label: &t!("dropdown-vpn-title"),
                    },
                },

                #[template]
                DropdownContent {
                    add_css_class: "network-content",
                    set_vexpand: true,

                    gtk::Label {
                        add_css_class: "network-connection-detail",
                        set_halign: gtk::Align::Start,
                        #[watch]
                        set_visible: model.operation_error.is_some(),
                        #[watch]
                        set_label: model.operation_error.as_deref().unwrap_or(""),
                    },

                    gtk::Label {
                        add_css_class: "section-label",
                        set_halign: gtk::Align::Start,
                        set_label: &t!("dropdown-vpn-active"),
                        #[watch]
                        set_visible: model.has_active,
                    },

                    #[template]
                    Card {
                        add_css_class: "network-connections-group",
                        set_orientation: gtk::Orientation::Vertical,
                        #[watch]
                        set_visible: model.has_active,

                        #[local_ref]
                        active_list_widget -> gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                        },
                    },

                    gtk::Label {
                        add_css_class: "section-label",
                        set_halign: gtk::Align::Start,
                        set_label: &t!("dropdown-vpn-available"),
                        #[watch]
                        set_visible: model.has_profiles,
                    },

                    #[template]
                    Card {
                        add_css_class: "network-list",
                        set_overflow: gtk::Overflow::Hidden,
                        set_vexpand: true,
                        #[watch]
                        set_visible: model.has_profiles,

                        gtk::ScrolledWindow {
                            add_css_class: "network-list-scroll",
                            set_vexpand: true,
                            set_hscrollbar_policy: gtk::PolicyType::Never,

                            #[local_ref]
                            profile_list_widget -> gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                            },
                        },
                    },

                    gtk::Box {
                        #[watch]
                        set_visible: !model.has_profiles && !model.has_active,

                        #[template]
                        EmptyState {
                            #[template_child]
                            icon {
                                add_css_class: "sm",
                                set_icon_name: Some("ld-shield-symbolic"),
                            },
                            #[template_child]
                            title {
                                set_label: &t!("dropdown-vpn-empty-title"),
                            },
                            #[template_child]
                            description {
                                set_label: &t!("dropdown-vpn-empty-description"),
                            },
                        },
                    },
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let active_list = FactoryVecDeque::builder()
            .launch(gtk::Box::default())
            .forward(sender.input_sender(), methods::forward_row_output);

        let profile_list = FactoryVecDeque::builder()
            .launch(gtk::Box::default())
            .forward(sender.input_sender(), methods::forward_row_output);

        let scale = init.config.config().styling.scale.get().value();
        watchers::spawn(&sender, &init.config, &init.network);

        let mut model = Self {
            network: init.network,
            scaled_width: scaled_dimension(BASE_WIDTH, scale),
            scaled_height: scaled_dimension(BASE_HEIGHT, scale),
            active_list,
            profile_list,
            has_active: false,
            has_profiles: false,
            operation_error: None,
        };

        model.rebuild_rows();

        let active_list_widget = model.active_list.widget();
        let profile_list_widget = model.profile_list.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        self.operation_error = None;

        match msg {
            VpnDropdownMsg::ConnectProfile(path) => {
                self.connect_profile(path, &sender);
            }
            VpnDropdownMsg::DisconnectActive(path) => {
                self.disconnect_active(path, &sender);
            }
            VpnDropdownMsg::TailscaleUp => {
                self.tailscale_up(&sender);
            }
            VpnDropdownMsg::TailscaleDown => {
                self.tailscale_down(&sender);
            }
        }
    }

    fn update_cmd(
        &mut self,
        msg: VpnDropdownCmd,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            VpnDropdownCmd::StateChanged => {
                self.rebuild_rows();
            }
            VpnDropdownCmd::ScaleChanged(scale) => {
                self.scaled_width = scaled_dimension(BASE_WIDTH, scale);
                self.scaled_height = scaled_dimension(BASE_HEIGHT, scale);
            }
            VpnDropdownCmd::OperationFailed(error) => {
                self.operation_error = Some(error);
                self.rebuild_rows();
            }
        }
    }
}
