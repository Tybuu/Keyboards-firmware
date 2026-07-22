use usbd_hid::descriptor::gen_hid_descriptor;
use usbd_hid::descriptor::{AsInputReport, generator_prelude::SerializedDescriptor};

#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = GENERIC_DESKTOP, usage = KEYBOARD) = {
        (usage_page = KEYBOARD, usage_min = 0xE0, usage_max = 0xE7) = {
            #[packed_bits = 8] #[item_settings(data,variable,absolute)] modifier=input;
        };
(usage_page = KEYBOARD, usage_min = 0x00, usage_max = 0x1F) = {
            #[packed_bits = 32] #[item_settings(data,variable,absolute)] nkro_0=input;
        };
        (usage_page = KEYBOARD, usage_min = 0x20, usage_max = 0x3F) = {
            #[packed_bits = 32] #[item_settings(data,variable,absolute)] nkro_1=input;
        };
        (usage_page = KEYBOARD, usage_min = 0x40, usage_max = 0x5F) = {
            #[packed_bits = 32] #[item_settings(data,variable,absolute)] nkro_2=input;
        };
        (usage_page = KEYBOARD, usage_min = 0x60, usage_max = 0x7F) = {
            #[packed_bits = 32] #[item_settings(data,variable,absolute)] nkro_3=input;
        };
        (usage_page = KEYBOARD, usage_min = 0x80, usage_max = 0x9F) = {
            #[packed_bits = 32] #[item_settings(data,variable,absolute)] nkro_4=input;
        };
        (usage_page = KEYBOARD, usage_min = 0xA0, usage_max = 0xBF) = {
            #[packed_bits = 32] #[item_settings(data,variable,absolute)] nkro_5=input;
        };
        (usage_page = KEYBOARD, usage_min = 0xC0, usage_max = 0xDF) = {
            #[packed_bits = 32] #[item_settings(data,variable,absolute)] nkro_6=input;
        };
    }
)]
#[allow(dead_code)]
#[derive(Default)]
pub struct KeyboardReportNKRO {
    pub modifier: u8,
    pub nkro_0: u32,
    pub nkro_1: u32,
    pub nkro_2: u32,
    pub nkro_3: u32,
    pub nkro_4: u32,
    pub nkro_5: u32,
    pub nkro_6: u32,
}

impl KeyboardReportNKRO {
    pub const fn default() -> Self {
        Self {
            modifier: 0,
            nkro_0: 0,
            nkro_1: 0,
            nkro_2: 0,
            nkro_3: 0,
            nkro_4: 0,
            nkro_5: 0,
            nkro_6: 0,
        }
    }
}

#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = GENERIC_DESKTOP, usage = MOUSE) = {
        (collection = PHYSICAL, usage = POINTER) = {
            (usage_page = BUTTON, usage_min = BUTTON_1, usage_max = BUTTON_8) = {
                #[packed_bits = 8] #[item_settings(data,variable,absolute)] buttons=input;
            };
            (usage_page = GENERIC_DESKTOP,) = {
                (usage = X,) = {
                    #[item_settings(data,variable,relative)] x=input;
                };
                (usage = Y,) = {
                    #[item_settings(data,variable,relative)] y=input;
                };
                (usage = WHEEL,) = {
                    #[item_settings(data,variable,relative)] wheel=input;
                };
            };
            (usage_page = CONSUMER,) = {
                (usage = AC_PAN,) = {
                    #[item_settings(data,variable,relative)] pan=input;
                };
            };
        };
    }
)]
#[allow(dead_code)]
#[derive(Default)]
pub struct MouseReport {
    pub buttons: u8,
    pub x: i8,
    pub y: i8,
    pub wheel: i8, // Scroll down (negative) or up (positive) this many units
    pub pan: i8,   // Scroll left (negative) or right (positive) this many units
}

#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = 0xFF69, usage = 0x01) = {
        input=input;
        output=output;
    }
)]
// The max for a single array is 32 elements
#[allow(dead_code)]
#[derive(Default)]
pub struct BufferReport {
    pub input: [u8; 32],
    pub output: [u8; 32],
}

#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = 0xFF69, usage = 0x02) = {
        input=input;
        output=output;
    }
)]
// The max for a single array is 32 elements
#[allow(dead_code)]
#[derive(Default)]
pub struct SlaveReport {
    pub input: [u8; 32],
    pub output: [u8; 32],
}
