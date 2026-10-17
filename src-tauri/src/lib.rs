//! WUHUICL Tauri library entry point.

pub mod app;
pub mod features;
pub mod ipc;

pub mod accounts;
pub mod appearance;
pub mod assetproto;
pub mod bridge;
pub mod catalog;
pub mod centercmd;
pub mod community;
pub mod direct;
pub mod download;
pub mod exportlogs;
pub mod frp;
pub mod gamedir;
pub mod gamefolders;
pub mod gameoptions;
pub mod gate;
pub mod installer;
pub mod javaman;
pub mod launch;
pub mod misc;
pub mod modpacks;
pub mod mods;
pub mod mslogin;
pub mod nbt;
pub mod net;
pub mod projections;
pub mod recordings;
pub mod selfupdate;
pub mod servers;
pub mod settings;
pub mod skins;
pub mod sysinfo;
pub mod tasks;
pub mod terracotta;
pub mod typednbt;
pub mod versions;
pub mod vl;
pub mod vlcli;
pub mod voxlink;
pub mod windowctl;
pub mod worlds;

pub use app::run;
