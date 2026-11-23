#![no_std]

pub const DONGLE_ADDRESS: u32 = 0x0A55_0A55;
pub const DONGLE_PREFIX: u8 = 0x42;
pub const KEYBOARD_ADDRESS: u32 = 0x0727_0727;
pub const LEFT_PREFIX: u8 = 0x21;
pub const RIGHT_PREFIX: u8 = 0x25;

pub mod key_config;
pub mod radio;
pub mod sensors;
