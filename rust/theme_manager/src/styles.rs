//! ThemeManager のスタイル索引(STYLE_*)と m_StyleList テーブル(機械生成)。
//!
//! src/ThemeManager.h の STYLE_* enum と src/ThemeManager.cpp の m_StyleList から生成。
#![allow(dead_code)]

use crate::{gradient_solid, StyleInfo};
use tvtest_color_scheme::indices::*;

// ----- スタイル索引(STYLE_*) -----
pub const STYLE_SCREEN: i32 = 0;
pub const STYLE_WINDOW_FRAME: i32 = 1;
pub const STYLE_WINDOW_ACTIVEFRAME: i32 = 2;
pub const STYLE_STATUSBAR: i32 = 3;
pub const STYLE_STATUSBAR_ITEM: i32 = 4;
pub const STYLE_STATUSBAR_BOTTOMITEM: i32 = 5;
pub const STYLE_STATUSBAR_ITEM_HOT: i32 = 6;
pub const STYLE_STATUSBAR_EVENT_PROGRESS: i32 = 7;
pub const STYLE_STATUSBAR_EVENT_PROGRESS_ELAPSED: i32 = 8;
pub const STYLE_TITLEBAR: i32 = 9;
pub const STYLE_TITLEBAR_CAPTION: i32 = 10;
pub const STYLE_TITLEBAR_BUTTON: i32 = 11;
pub const STYLE_TITLEBAR_BUTTON_HOT: i32 = 12;
pub const STYLE_SIDEBAR: i32 = 13;
pub const STYLE_SIDEBAR_ITEM: i32 = 14;
pub const STYLE_SIDEBAR_ITEM_HOT: i32 = 15;
pub const STYLE_SIDEBAR_ITEM_CHECKED: i32 = 16;
pub const STYLE_PANEL_TAB: i32 = 17;
pub const STYLE_PANEL_CURTAB: i32 = 18;
pub const STYLE_PANEL_TABMARGIN: i32 = 19;
pub const STYLE_PANEL_TITLE: i32 = 20;
pub const STYLE_PANEL_CONTENT: i32 = 21;
pub const STYLE_INFORMATIONPANEL_EVENTINFO: i32 = 22;
pub const STYLE_INFORMATIONPANEL_BUTTON: i32 = 23;
pub const STYLE_INFORMATIONPANEL_BUTTON_HOT: i32 = 24;
pub const STYLE_PROGRAMLISTPANEL_CHANNEL: i32 = 25;
pub const STYLE_PROGRAMLISTPANEL_CURCHANNEL: i32 = 26;
pub const STYLE_PROGRAMLISTPANEL_CHANNELBUTTON: i32 = 27;
pub const STYLE_PROGRAMLISTPANEL_CHANNELBUTTON_HOT: i32 = 28;
pub const STYLE_PROGRAMLISTPANEL_EVENT: i32 = 29;
pub const STYLE_PROGRAMLISTPANEL_CUREVENT: i32 = 30;
pub const STYLE_PROGRAMLISTPANEL_TITLE: i32 = 31;
pub const STYLE_PROGRAMLISTPANEL_CURTITLE: i32 = 32;
pub const STYLE_CHANNELPANEL_CHANNELNAME: i32 = 33;
pub const STYLE_CHANNELPANEL_CURCHANNELNAME: i32 = 34;
pub const STYLE_CHANNELPANEL_EVENTNAME1: i32 = 35;
pub const STYLE_CHANNELPANEL_EVENTNAME2: i32 = 36;
pub const STYLE_CHANNELPANEL_CURCHANNELEVENTNAME1: i32 = 37;
pub const STYLE_CHANNELPANEL_CURCHANNELEVENTNAME2: i32 = 38;
pub const STYLE_CHANNELPANEL_FEATUREDMARK: i32 = 39;
pub const STYLE_CHANNELPANEL_PROGRESS: i32 = 40;
pub const STYLE_CHANNELPANEL_CURPROGRESS: i32 = 41;
pub const STYLE_CONTROLPANEL_ITEM: i32 = 42;
pub const STYLE_CONTROLPANEL_ITEM_HOT: i32 = 43;
pub const STYLE_CONTROLPANEL_ITEM_CHECKED: i32 = 44;
pub const STYLE_NOTIFICATIONBAR: i32 = 45;
pub const STYLE_PROGRAMGUIDE_FEATUREDMARK: i32 = 46;
pub const STYLE_PROGRAMGUIDE_CHANNEL: i32 = 47;
pub const STYLE_PROGRAMGUIDE_CURCHANNEL: i32 = 48;
pub const STYLE_PROGRAMGUIDE_TIMEBAR: i32 = 49;
pub const STYLE_PROGRAMGUIDE_TIMEBAR_0_2: i32 = 50;
pub const STYLE_PROGRAMGUIDE_TIMEBAR_3_5: i32 = 51;
pub const STYLE_PROGRAMGUIDE_TIMEBAR_6_8: i32 = 52;
pub const STYLE_PROGRAMGUIDE_TIMEBAR_9_11: i32 = 53;
pub const STYLE_PROGRAMGUIDE_TIMEBAR_12_14: i32 = 54;
pub const STYLE_PROGRAMGUIDE_TIMEBAR_15_17: i32 = 55;
pub const STYLE_PROGRAMGUIDE_TIMEBAR_18_20: i32 = 56;
pub const STYLE_PROGRAMGUIDE_TIMEBAR_21_23: i32 = 57;
pub const STYLE_PROGRAMGUIDE_STATUS: i32 = 58;
pub const STYLE_PROGRAMGUIDE_DATEBUTTON: i32 = 59;
pub const STYLE_PROGRAMGUIDE_DATEBUTTON_CUR: i32 = 60;
pub const STYLE_PROGRAMGUIDE_DATEBUTTON_HOT: i32 = 61;
pub const STYLE_PROGRAMGUIDE_TIMEBUTTON: i32 = 62;
pub const STYLE_PROGRAMGUIDE_TIMEBUTTON_CUR: i32 = 63;
pub const STYLE_PROGRAMGUIDE_TIMEBUTTON_HOT: i32 = 64;
pub const STYLE_PROGRAMGUIDE_FAVORITEBUTTON: i32 = 65;
pub const STYLE_PROGRAMGUIDE_FAVORITEBUTTON_CUR: i32 = 66;
pub const STYLE_PROGRAMGUIDE_FAVORITEBUTTON_HOT: i32 = 67;
pub const NUM_STYLES: i32 = 68;

// ----- スタイル定義表(ThemeManager.cpp m_StyleList) -----
pub static STYLE_LIST: [StyleInfo; 68] = [
    StyleInfo { name: "screen", gradient: -1, border: BORDER_SCREEN, fore_color: -1 },
    StyleInfo { name: "window.frame", gradient: gradient_solid(COLOR_WINDOWFRAMEBACK), border: BORDER_WINDOWFRAME, fore_color: -1 },
    StyleInfo { name: "window.frame.active", gradient: gradient_solid(COLOR_WINDOWACTIVEFRAMEBACK), border: BORDER_WINDOWACTIVEFRAME, fore_color: -1 },
    StyleInfo { name: "status-bar", gradient: -1, border: BORDER_STATUS, fore_color: -1 },
    StyleInfo { name: "status-bar.item", gradient: GRADIENT_STATUSBACK, border: BORDER_STATUSITEM, fore_color: COLOR_STATUSTEXT },
    StyleInfo { name: "status-bar.item.bottom", gradient: GRADIENT_STATUSBOTTOMITEMBACK, border: BORDER_STATUSBOTTOMITEM, fore_color: COLOR_STATUSBOTTOMITEMTEXT },
    StyleInfo { name: "status-bar.item.hot", gradient: GRADIENT_STATUSHIGHLIGHTBACK, border: BORDER_STATUSHIGHLIGHT, fore_color: COLOR_STATUSHIGHLIGHTTEXT },
    StyleInfo { name: "status-bar.event.progress", gradient: GRADIENT_STATUSEVENTPROGRESSBACK, border: BORDER_STATUSEVENTPROGRESS, fore_color: -1 },
    StyleInfo { name: "status-bar.event.progress.elapsed", gradient: GRADIENT_STATUSEVENTPROGRESSELAPSED, border: BORDER_STATUSEVENTPROGRESSELAPSED, fore_color: -1 },
    StyleInfo { name: "title-bar", gradient: -1, border: BORDER_TITLEBAR, fore_color: -1 },
    StyleInfo { name: "title-bar.caption", gradient: GRADIENT_TITLEBARBACK, border: BORDER_TITLEBARCAPTION, fore_color: COLOR_TITLEBARTEXT },
    StyleInfo { name: "title-bar.icon", gradient: GRADIENT_TITLEBARICON, border: BORDER_TITLEBARICON, fore_color: COLOR_TITLEBARICON },
    StyleInfo { name: "title-bar.icon.hot", gradient: GRADIENT_TITLEBARHIGHLIGHTBACK, border: BORDER_TITLEBARHIGHLIGHT, fore_color: COLOR_TITLEBARHIGHLIGHTICON },
    StyleInfo { name: "side-bar", gradient: -1, border: BORDER_SIDEBAR, fore_color: -1 },
    StyleInfo { name: "side-bar.item", gradient: GRADIENT_SIDEBARBACK, border: BORDER_SIDEBARITEM, fore_color: COLOR_SIDEBARICON },
    StyleInfo { name: "side-bar.item.hot", gradient: GRADIENT_SIDEBARHIGHLIGHTBACK, border: BORDER_SIDEBARHIGHLIGHT, fore_color: COLOR_SIDEBARHIGHLIGHTICON },
    StyleInfo { name: "side-bar.item.checked", gradient: GRADIENT_SIDEBARCHECKBACK, border: BORDER_SIDEBARCHECK, fore_color: COLOR_SIDEBARCHECKICON },
    StyleInfo { name: "panel.tab", gradient: GRADIENT_PANELTABBACK, border: BORDER_PANEL_TAB, fore_color: COLOR_PANELTABTEXT },
    StyleInfo { name: "panel.tab.current", gradient: GRADIENT_PANELCURTABBACK, border: BORDER_PANEL_CURTAB, fore_color: COLOR_PANELCURTABTEXT },
    StyleInfo { name: "panel.tab.margin", gradient: GRADIENT_PANELTABMARGIN, border: BORDER_PANEL_TABMARGIN, fore_color: -1 },
    StyleInfo { name: "panel.title", gradient: GRADIENT_PANELTITLEBACK, border: BORDER_PANEL_TITLE, fore_color: COLOR_PANELTITLETEXT },
    StyleInfo { name: "panel.content", gradient: gradient_solid(COLOR_PANELBACK), border: -1, fore_color: COLOR_PANELTEXT },
    StyleInfo { name: "information-panel.event", gradient: gradient_solid(COLOR_PROGRAMINFOBACK), border: BORDER_INFORMATIONPANEL_EVENTINFO, fore_color: COLOR_PROGRAMINFOTEXT },
    StyleInfo { name: "information-panel.button", gradient: GRADIENT_INFORMATIONPANEL_BUTTONBACK, border: BORDER_INFORMATIONPANEL_BUTTON, fore_color: COLOR_INFORMATIONPANEL_BUTTONTEXT },
    StyleInfo { name: "information-panel.button.hot", gradient: GRADIENT_INFORMATIONPANEL_HOTBUTTONBACK, border: BORDER_INFORMATIONPANEL_HOTBUTTON, fore_color: COLOR_INFORMATIONPANEL_HOTBUTTONTEXT },
    StyleInfo { name: "program-list-panel.channel", gradient: GRADIENT_PROGRAMLISTPANEL_CHANNELBACK, border: BORDER_PROGRAMLISTPANEL_CHANNEL, fore_color: COLOR_PROGRAMLISTPANEL_CHANNELTEXT },
    StyleInfo { name: "program-list-panel.channel.current", gradient: GRADIENT_PROGRAMLISTPANEL_CURCHANNELBACK, border: BORDER_PROGRAMLISTPANEL_CURCHANNEL, fore_color: COLOR_PROGRAMLISTPANEL_CURCHANNELTEXT },
    StyleInfo { name: "program-list-panel.channel.button", gradient: GRADIENT_PROGRAMLISTPANEL_CHANNELBUTTONBACK, border: BORDER_PROGRAMLISTPANEL_CHANNELBUTTON, fore_color: COLOR_PROGRAMLISTPANEL_CHANNELBUTTONTEXT },
    StyleInfo { name: "program-list-panel.channel.button.hot", gradient: GRADIENT_PROGRAMLISTPANEL_CHANNELBUTTONHOTBACK, border: BORDER_PROGRAMLISTPANEL_CHANNELBUTTONHOT, fore_color: COLOR_PROGRAMLISTPANEL_CHANNELBUTTONHOTTEXT },
    StyleInfo { name: "program-list-panel.event", gradient: GRADIENT_PROGRAMLISTPANEL_EVENTBACK, border: BORDER_PROGRAMLISTPANEL_EVENT, fore_color: COLOR_PROGRAMLISTPANEL_EVENTTEXT },
    StyleInfo { name: "program-list-panel.event.current", gradient: GRADIENT_PROGRAMLISTPANEL_CUREVENTBACK, border: BORDER_PROGRAMLISTPANEL_CUREVENT, fore_color: COLOR_PROGRAMLISTPANEL_CUREVENTTEXT },
    StyleInfo { name: "program-list-panel.title", gradient: GRADIENT_PROGRAMLISTPANEL_TITLEBACK, border: BORDER_PROGRAMLISTPANEL_TITLE, fore_color: COLOR_PROGRAMLISTPANEL_TITLETEXT },
    StyleInfo { name: "program-list-panel.title.current", gradient: GRADIENT_PROGRAMLISTPANEL_CURTITLEBACK, border: BORDER_PROGRAMLISTPANEL_CURTITLE, fore_color: COLOR_PROGRAMLISTPANEL_CURTITLETEXT },
    StyleInfo { name: "channel-list-panel.channel-name", gradient: GRADIENT_CHANNELPANEL_CHANNELNAMEBACK, border: BORDER_CHANNELPANEL_CHANNELNAME, fore_color: COLOR_CHANNELPANEL_CHANNELNAMETEXT },
    StyleInfo { name: "channel-list-panel.channel-name.current", gradient: GRADIENT_CHANNELPANEL_CURCHANNELNAMEBACK, border: BORDER_CHANNELPANEL_CURCHANNELNAME, fore_color: COLOR_CHANNELPANEL_CURCHANNELNAMETEXT },
    StyleInfo { name: "channel-list-panel.event-name", gradient: GRADIENT_CHANNELPANEL_EVENTNAMEBACK1, border: BORDER_CHANNELPANEL_EVENTNAME1, fore_color: COLOR_CHANNELPANEL_EVENTNAME1TEXT },
    StyleInfo { name: "channel-list-panel.event-name.odd", gradient: GRADIENT_CHANNELPANEL_EVENTNAMEBACK2, border: BORDER_CHANNELPANEL_EVENTNAME2, fore_color: COLOR_CHANNELPANEL_EVENTNAME2TEXT },
    StyleInfo { name: "channel-list-panel.event-name.current", gradient: GRADIENT_CHANNELPANEL_CUREVENTNAMEBACK1, border: BORDER_CHANNELPANEL_CUREVENTNAME1, fore_color: COLOR_CHANNELPANEL_CUREVENTNAME1TEXT },
    StyleInfo { name: "channel-list-panel.event-name.current.odd", gradient: GRADIENT_CHANNELPANEL_CUREVENTNAMEBACK2, border: BORDER_CHANNELPANEL_CUREVENTNAME2, fore_color: COLOR_CHANNELPANEL_CUREVENTNAME2TEXT },
    StyleInfo { name: "channel-list-panel.featured-mark", gradient: GRADIENT_CHANNELPANEL_FEATUREDMARK, border: BORDER_CHANNELPANEL_FEATUREDMARK, fore_color: -1 },
    StyleInfo { name: "channel-list-panel.progress", gradient: GRADIENT_CHANNELPANEL_PROGRESS, border: BORDER_CHANNELPANEL_PROGRESS, fore_color: -1 },
    StyleInfo { name: "channel-list-panel.progress.current", gradient: GRADIENT_CHANNELPANEL_CURPROGRESS, border: BORDER_CHANNELPANEL_CURPROGRESS, fore_color: -1 },
    StyleInfo { name: "control-panel.item", gradient: GRADIENT_CONTROLPANELBACK, border: BORDER_CONTROLPANELITEM, fore_color: COLOR_CONTROLPANELTEXT },
    StyleInfo { name: "control-panel.item.hot", gradient: GRADIENT_CONTROLPANELHIGHLIGHTBACK, border: BORDER_CONTROLPANELHIGHLIGHTITEM, fore_color: COLOR_CONTROLPANELHIGHLIGHTTEXT },
    StyleInfo { name: "control-panel.item.checked", gradient: GRADIENT_CONTROLPANELCHECKEDBACK, border: BORDER_CONTROLPANELCHECKEDITEM, fore_color: COLOR_CONTROLPANELCHECKEDTEXT },
    StyleInfo { name: "notification-bar", gradient: GRADIENT_NOTIFICATIONBARBACK, border: -1, fore_color: COLOR_NOTIFICATIONBARTEXT },
    StyleInfo { name: "program-guide.event.featured-mark", gradient: GRADIENT_PROGRAMGUIDE_FEATUREDMARK, border: BORDER_PROGRAMGUIDE_FEATUREDMARK, fore_color: -1 },
    StyleInfo { name: "program-guide.header.channel-name", gradient: GRADIENT_PROGRAMGUIDECHANNELBACK, border: -1, fore_color: COLOR_PROGRAMGUIDE_CHANNELTEXT },
    StyleInfo { name: "program-guide.header.channel-name.current", gradient: GRADIENT_PROGRAMGUIDECURCHANNELBACK, border: -1, fore_color: COLOR_PROGRAMGUIDE_CURCHANNELTEXT },
    StyleInfo { name: "program-guide.time-bar", gradient: GRADIENT_PROGRAMGUIDETIMEBACK, border: -1, fore_color: COLOR_PROGRAMGUIDE_TIMETEXT },
    StyleInfo { name: "program-guide.time-bar.time-0-2", gradient: GRADIENT_PROGRAMGUIDETIME0TO2BACK, border: -1, fore_color: COLOR_PROGRAMGUIDE_TIMETEXT },
    StyleInfo { name: "program-guide.time-bar.time-3-5", gradient: GRADIENT_PROGRAMGUIDETIME3TO5BACK, border: -1, fore_color: COLOR_PROGRAMGUIDE_TIMETEXT },
    StyleInfo { name: "program-guide.time-bar.time-6-8", gradient: GRADIENT_PROGRAMGUIDETIME6TO8BACK, border: -1, fore_color: COLOR_PROGRAMGUIDE_TIMETEXT },
    StyleInfo { name: "program-guide.time-bar.time-9-11", gradient: GRADIENT_PROGRAMGUIDETIME9TO11BACK, border: -1, fore_color: COLOR_PROGRAMGUIDE_TIMETEXT },
    StyleInfo { name: "program-guide.time-bar.time-12-14", gradient: GRADIENT_PROGRAMGUIDETIME12TO14BACK, border: -1, fore_color: COLOR_PROGRAMGUIDE_TIMETEXT },
    StyleInfo { name: "program-guide.time-bar.time-15-17", gradient: GRADIENT_PROGRAMGUIDETIME15TO17BACK, border: -1, fore_color: COLOR_PROGRAMGUIDE_TIMETEXT },
    StyleInfo { name: "program-guide.time-bar.time-18-20", gradient: GRADIENT_PROGRAMGUIDETIME18TO20BACK, border: -1, fore_color: COLOR_PROGRAMGUIDE_TIMETEXT },
    StyleInfo { name: "program-guide.time-bar.time-21-23", gradient: GRADIENT_PROGRAMGUIDETIME21TO23BACK, border: -1, fore_color: COLOR_PROGRAMGUIDE_TIMETEXT },
    StyleInfo { name: "program-guide.status-bar", gradient: -1, border: BORDER_PROGRAMGUIDESTATUS, fore_color: -1 },
    StyleInfo { name: "program-guide.date-button", gradient: GRADIENT_PROGRAMGUIDE_DATEBUTTON_BACK, border: BORDER_PROGRAMGUIDE_DATEBUTTON, fore_color: COLOR_PROGRAMGUIDE_DATEBUTTON_TEXT },
    StyleInfo { name: "program-guide.date-button.current", gradient: GRADIENT_PROGRAMGUIDE_DATEBUTTON_CURBACK, border: BORDER_PROGRAMGUIDE_DATEBUTTON_CUR, fore_color: COLOR_PROGRAMGUIDE_DATEBUTTON_CURTEXT },
    StyleInfo { name: "program-guide.date-button.hot", gradient: GRADIENT_PROGRAMGUIDE_DATEBUTTON_HOTBACK, border: BORDER_PROGRAMGUIDE_DATEBUTTON_HOT, fore_color: COLOR_PROGRAMGUIDE_DATEBUTTON_HOTTEXT },
    StyleInfo { name: "program-guide.time-button", gradient: -1, border: BORDER_PROGRAMGUIDE_TIMEBUTTON, fore_color: -1 },
    StyleInfo { name: "program-guide.time-button.current", gradient: -1, border: BORDER_PROGRAMGUIDE_TIMEBUTTON_CUR, fore_color: -1 },
    StyleInfo { name: "program-guide.time-button.hot", gradient: -1, border: BORDER_PROGRAMGUIDE_TIMEBUTTON_HOT, fore_color: -1 },
    StyleInfo { name: "program-guide.favorite-button", gradient: GRADIENT_PROGRAMGUIDE_FAVORITEBUTTON_BACK, border: BORDER_PROGRAMGUIDE_FAVORITEBUTTON, fore_color: -1 },
    StyleInfo { name: "program-guide.favorite-button.current", gradient: GRADIENT_PROGRAMGUIDE_FAVORITEBUTTON_CURBACK, border: BORDER_PROGRAMGUIDE_FAVORITEBUTTON_CUR, fore_color: -1 },
    StyleInfo { name: "program-guide.favorite-button.hot", gradient: GRADIENT_PROGRAMGUIDE_FAVORITEBUTTON_HOTBACK, border: BORDER_PROGRAMGUIDE_FAVORITEBUTTON_HOT, fore_color: -1 },
];

