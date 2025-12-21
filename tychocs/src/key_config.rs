use key_lib::{
    codes::ScanCodeBehavior::*,
    keys::{ConfigIndicator, Keys},
    scan_codes::KeyCodes::*,
};

pub fn set_keys(keys: &mut Keys<impl ConfigIndicator>) {
    // Layer 0
    keys.set_code(Single(KeyboardQq), 0, 0);
    keys.set_code(Single(KeyboardWw), 1, 0);
    keys.set_code(Single(KeyboardEe), 2, 0);
    keys.set_code(Single(KeyboardRr), 3, 0);
    keys.set_code(Single(KeyboardTt), 4, 0);

    keys.set_code(Single(KeyboardAa), 5, 0);
    keys.set_code(Single(KeyboardSs), 6, 0);
    keys.set_code(Single(KeyboardDd), 7, 0);
    keys.set_code(Single(KeyboardFf), 8, 0);
    keys.set_code(Single(KeyboardGg), 9, 0);

    keys.set_code(Single(KeyboardZz), 10, 0);
    keys.set_code(Single(KeyboardXx), 11, 0);
    keys.set_code(Single(KeyboardCc), 12, 0);
    keys.set_code(Single(KeyboardVv), 13, 0);
    keys.set_code(Single(KeyboardBb), 14, 0);

    keys.set_code(Single(Layer4), 15, 0);
    keys.set_code(
        CombinedKey {
            other_index: 34,
            normal_code: Layer1,
            combined_code: Layer3,
        },
        16,
        0,
    );
    keys.set_code(Single(KeyboardSpacebar), 17, 0);

    keys.set_code(Single(KeyboardYy), 18, 0);
    keys.set_code(Single(KeyboardUu), 19, 0);
    keys.set_code(Single(KeyboardIi), 20, 0);
    keys.set_code(Single(KeyboardOo), 21, 0);
    keys.set_code(Single(KeyboardPp), 22, 0);

    keys.set_code(Single(KeyboardHh), 23, 0);
    keys.set_code(Single(KeyboardJj), 24, 0);
    keys.set_code(Single(KeyboardKk), 25, 0);
    keys.set_code(Single(KeyboardLl), 26, 0);
    keys.set_code(Single(KeyboardSemiColon), 27, 0);

    keys.set_code(Single(KeyboardNn), 28, 0);
    keys.set_code(Single(KeyboardMm), 29, 0);
    keys.set_code(Single(KeyboardCommaLess), 30, 0);
    keys.set_code(Single(KeyboardPeriodGreater), 31, 0);
    keys.set_code(Single(KeyboardSlashQuestion), 32, 0);

    keys.set_code(Single(KeyboardLeftShift), 33, 0);
    keys.set_code(
        CombinedKey {
            other_index: 16,
            normal_code: Layer2,
            combined_code: Layer4,
        },
        34,
        0,
    );
    keys.set_code(Single(Layer5), 35, 0);

    // Layer 1
    keys.set_code(Single(KeyboardTab), 0, 1);
    keys.set_code(Single(KeyboardCommaLess), 1, 1);
    keys.set_code(Single(KeyboardPeriodGreater), 2, 1);
    keys.set_code(Single(KeyboardSlashQuestion), 3, 1);
    keys.set_code(Single(KeyboardVolumeUp), 4, 1);

    keys.set_code(Single(KeyboardLeftGUI), 5, 1);
    keys.set_code(Single(KeyboardLeftAlt), 6, 1);
    keys.set_code(Single(KeyboardLeftControl), 7, 1);
    keys.set_code(Single(KeyboardLeftShift), 8, 1);
    keys.set_code(Single(KeyboardVolumeDown), 9, 1);

    keys.set_code(Single(MouseScrollNeg), 10, 1);
    keys.set_code(Single(MouseScrollPos), 11, 1);
    keys.set_code(Single(MouseLeftClick), 12, 1);
    keys.set_code(Single(MouseMiddleClick), 13, 1);
    keys.set_code(Single(MouseRightClick), 14, 1);

    keys.set_code(Single(Layer4), 15, 1);
    keys.set_code(
        CombinedKey {
            other_index: 34,
            normal_code: Layer1,
            combined_code: Layer3,
        },
        16,
        1,
    );
    keys.set_code(Single(KeyboardSpacebar), 17, 1);

    keys.set_code(Single(KeyboardCapsLock), 18, 1);
    keys.set_code(Single(KeyboardDelete), 22, 1);

    keys.set_code(Single(KeyboardLeftArrow), 23, 1);
    keys.set_code(Single(KeyboardDownArrow), 24, 1);
    keys.set_code(Single(KeyboardUpArrow), 25, 1);
    keys.set_code(Single(KeyboardRightArrow), 26, 1);
    keys.set_code(Single(KeyboardBackspace), 27, 1);

    keys.set_code(Single(MouseXNeg), 28, 1);
    keys.set_code(Single(MouseYPos), 29, 1);
    keys.set_code(Single(MouseYNeg), 30, 1);
    keys.set_code(Single(MouseXPos), 31, 1);
    keys.set_code(Single(KeyboardEnter), 32, 1);

    keys.set_code(Single(KeyboardLeftShift), 33, 1);
    keys.set_code(
        CombinedKey {
            other_index: 16,
            normal_code: Layer2,
            combined_code: Layer4,
        },
        34,
        1,
    );
    keys.set_code(Single(Layer5), 35, 1);

    // Layer 2
    keys.set_code(Single(KeyboardEscape), 0, 2);
    keys.set_code(Single(KeyboardOpenBracketBrace), 1, 2);
    keys.set_code(Double(KeyboardLeftShift, KeyboardOpenBracketBrace), 2, 2);
    keys.set_code(Double(KeyboardLeftShift, Keyboard9OpenParens), 3, 2);
    keys.set_code(Double(KeyboardLeftShift, KeyboardBacktickTilde), 4, 2);

    keys.set_code(Single(KeyboardDashUnderscore), 5, 2);
    keys.set_code(Double(KeyboardLeftShift, Keyboard8Asterisk), 6, 2);
    keys.set_code(Single(KeyboardEqualPlus), 7, 2);
    keys.set_code(Double(KeyboardLeftShift, KeyboardDashUnderscore), 8, 2);
    keys.set_code(Double(KeyboardLeftShift, Keyboard4Dollar), 9, 2);

    keys.set_code(Double(KeyboardLeftShift, KeyboardEqualPlus), 10, 2);
    keys.set_code(Double(KeyboardLeftShift, KeyboardBackslashBar), 11, 2);
    keys.set_code(Double(KeyboardLeftShift, Keyboard2At), 12, 2);
    keys.set_code(Single(KeyboardSingleDoubleQuote), 13, 2);
    keys.set_code(Double(KeyboardLeftShift, Keyboard5Percent), 14, 2);

    keys.set_code(Single(Layer4), 15, 2);
    keys.set_code(
        CombinedKey {
            other_index: 34,
            normal_code: Layer1,
            combined_code: Layer3,
        },
        16,
        2,
    );
    keys.set_code(Single(KeyboardSpacebar), 17, 2);

    keys.set_code(Double(KeyboardLeftShift, Keyboard6Caret), 18, 2);
    keys.set_code(Double(KeyboardLeftShift, Keyboard0CloseParens), 19, 2);
    keys.set_code(Double(KeyboardLeftShift, KeyboardCloseBracketBrace), 20, 2);
    keys.set_code(Single(KeyboardCloseBracketBrace), 21, 2);
    keys.set_code(Single(KeyboardBacktickTilde), 22, 2);

    keys.set_code(Double(KeyboardLeftShift, Keyboard3Hash), 23, 2);
    keys.set_code(Single(KeyboardRightShift), 24, 2);
    keys.set_code(Single(KeyboardRightControl), 25, 2);
    keys.set_code(Single(KeyboardRightAlt), 26, 2);
    keys.set_code(Single(KeyboardRightGUI), 27, 2);

    keys.set_code(Single(KeyboardBackslashBar), 29, 2);
    keys.set_code(Double(KeyboardLeftShift, Keyboard7Ampersand), 30, 2);
    keys.set_code(Double(KeyboardLeftShift, KeyboardSingleDoubleQuote), 31, 2);
    keys.set_code(Double(KeyboardLeftShift, Keyboard1Exclamation), 32, 2);

    keys.set_code(Single(KeyboardLeftShift), 33, 2);
    keys.set_code(
        CombinedKey {
            other_index: 16,
            normal_code: Layer2,
            combined_code: Layer4,
        },
        34,
        2,
    );
    keys.set_code(Single(Layer5), 35, 2);

    // Layer 3
    keys.set_code(Single(Keyboard1Exclamation), 0, 3);
    keys.set_code(Single(Keyboard2At), 1, 3);
    keys.set_code(Single(Keyboard3Hash), 2, 3);
    keys.set_code(Single(Keyboard4Dollar), 3, 3);
    keys.set_code(Single(Keyboard5Percent), 4, 3);

    keys.set_code(Single(KeyboardLeftGUI), 5, 3);
    keys.set_code(Single(KeyboardLeftAlt), 6, 3);
    keys.set_code(Single(KeyboardLeftControl), 7, 3);
    keys.set_code(Single(KeyboardLeftShift), 8, 3);
    keys.set_code(Single(KeyboardF11), 9, 3);

    keys.set_code(Single(KeyboardF1), 10, 3);
    keys.set_code(Single(KeyboardF2), 11, 3);
    keys.set_code(Single(KeyboardF3), 12, 3);
    keys.set_code(Single(KeyboardF4), 13, 3);
    keys.set_code(Single(KeyboardF5), 14, 3);

    keys.set_code(Single(Layer4), 15, 3);
    keys.set_code(
        CombinedKey {
            other_index: 34,
            normal_code: Layer1,
            combined_code: Layer3,
        },
        16,
        3,
    );
    keys.set_code(Single(KeyboardSpacebar), 17, 3);

    keys.set_code(Single(Keyboard6Caret), 18, 3);
    keys.set_code(Single(Keyboard7Ampersand), 19, 3);
    keys.set_code(Single(Keyboard8Asterisk), 20, 3);
    keys.set_code(Single(Keyboard9OpenParens), 21, 3);
    keys.set_code(Single(Keyboard0CloseParens), 22, 3);

    keys.set_code(Single(KeyboardF12), 23, 3);
    keys.set_code(Single(KeyboardRightShift), 24, 3);
    keys.set_code(Single(KeyboardRightControl), 25, 3);
    keys.set_code(Single(KeyboardRightAlt), 26, 3);
    keys.set_code(Single(KeyboardRightGUI), 27, 3);

    keys.set_code(Single(KeyboardF6), 28, 3);
    keys.set_code(Single(KeyboardF7), 29, 3);
    keys.set_code(Single(KeyboardF8), 30, 3);
    keys.set_code(Single(KeyboardF9), 31, 3);
    keys.set_code(Single(KeyboardF10), 32, 3);

    keys.set_code(Single(KeyboardLeftShift), 33, 3);
    keys.set_code(
        CombinedKey {
            other_index: 16,
            normal_code: Layer2,
            combined_code: Layer4,
        },
        34,
        3,
    );
    keys.set_code(Single(Layer5), 35, 3);
}
