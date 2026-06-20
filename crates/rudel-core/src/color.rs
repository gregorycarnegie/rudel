// color.rs - CSS named-color table and hex/color -> number conversion.
// Ported sample-for-sample from strudel/packages/draw/color.mjs: the `colorMap`
// of CSS named colors to hex, `convertHexToNumber` (parseInt of the hex digits),
// and `convertColorToNumber` (lowercase, then hex passthrough / named lookup /
// -1 on an unrecognized color). Rudel consumes this where patterns set a `color`
// control that visuals render (see the inline draw widgets in rudel-app).
//
// Intentional difference: Strudel's `convertHexToNumber` is a thin wrapper over
// JS `parseInt(hex, 16)`, which yields `NaN` for a non-hex string. NaN has no
// integer representation, so `convert_hex_to_number` returns -1 for an
// unparseable hex (matching the "unrecognized" sentinel `convert_color_to_number`
// already returns), rather than a sentinel that pretends to be a number.
// SPDX-License-Identifier: AGPL-3.0-or-later

/// CSS named colors mapped to their `#rrggbb` hex, in upstream `colorMap` order.
pub static COLOR_MAP: phf::OrderedMap<&'static str, &'static str> = phf::phf_ordered_map! {
    "aliceblue" => "#f0f8ff",
    "antiquewhite" => "#faebd7",
    "aqua" => "#00ffff",
    "aquamarine" => "#7fffd4",
    "azure" => "#f0ffff",
    "beige" => "#f5f5dc",
    "bisque" => "#ffe4c4",
    "black" => "#000000",
    "blanchedalmond" => "#ffebcd",
    "blue" => "#0000ff",
    "blueviolet" => "#8a2be2",
    "brown" => "#a52a2a",
    "burlywood" => "#deb887",
    "cadetblue" => "#5f9ea0",
    "chartreuse" => "#7fff00",
    "chocolate" => "#d2691e",
    "coral" => "#ff7f50",
    "cornflowerblue" => "#6495ed",
    "cornsilk" => "#fff8dc",
    "crimson" => "#dc143c",
    "cyan" => "#00ffff",
    "darkblue" => "#00008b",
    "darkcyan" => "#008b8b",
    "darkgoldenrod" => "#b8860b",
    "darkgray" => "#a9a9a9",
    "darkgreen" => "#006400",
    "darkgrey" => "#a9a9a9",
    "darkkhaki" => "#bdb76b",
    "darkmagenta" => "#8b008b",
    "darkolivegreen" => "#556b2f",
    "darkorange" => "#ff8c00",
    "darkorchid" => "#9932cc",
    "darkred" => "#8b0000",
    "darksalmon" => "#e9967a",
    "darkseagreen" => "#8fbc8f",
    "darkslateblue" => "#483d8b",
    "darkslategray" => "#2f4f4f",
    "darkslategrey" => "#2f4f4f",
    "darkturquoise" => "#00ced1",
    "darkviolet" => "#9400d3",
    "deeppink" => "#ff1493",
    "deepskyblue" => "#00bfff",
    "dimgray" => "#696969",
    "dimgrey" => "#696969",
    "dodgerblue" => "#1e90ff",
    "firebrick" => "#b22222",
    "floralwhite" => "#fffaf0",
    "forestgreen" => "#228b22",
    "fuchsia" => "#ff00ff",
    "gainsboro" => "#dcdcdc",
    "ghostwhite" => "#f8f8ff",
    "gold" => "#ffd700",
    "goldenrod" => "#daa520",
    "gray" => "#808080",
    "green" => "#008000",
    "greenyellow" => "#adff2f",
    "grey" => "#808080",
    "honeydew" => "#f0fff0",
    "hotpink" => "#ff69b4",
    "indianred" => "#cd5c5c",
    "indigo" => "#4b0082",
    "ivory" => "#fffff0",
    "khaki" => "#f0e68c",
    "lavender" => "#e6e6fa",
    "lavenderblush" => "#fff0f5",
    "lawngreen" => "#7cfc00",
    "lemonchiffon" => "#fffacd",
    "lightblue" => "#add8e6",
    "lightcoral" => "#f08080",
    "lightcyan" => "#e0ffff",
    "lightgoldenrodyellow" => "#fafad2",
    "lightgray" => "#d3d3d3",
    "lightgreen" => "#90ee90",
    "lightgrey" => "#d3d3d3",
    "lightpink" => "#ffb6c1",
    "lightsalmon" => "#ffa07a",
    "lightseagreen" => "#20b2aa",
    "lightskyblue" => "#87cefa",
    "lightslategray" => "#778899",
    "lightslategrey" => "#778899",
    "lightsteelblue" => "#b0c4de",
    "lightyellow" => "#ffffe0",
    "lime" => "#00ff00",
    "limegreen" => "#32cd32",
    "linen" => "#faf0e6",
    "magenta" => "#ff00ff",
    "maroon" => "#800000",
    "mediumaquamarine" => "#66cdaa",
    "mediumblue" => "#0000cd",
    "mediumorchid" => "#ba55d3",
    "mediumpurple" => "#9370db",
    "mediumseagreen" => "#3cb371",
    "mediumslateblue" => "#7b68ee",
    "mediumspringgreen" => "#00fa9a",
    "mediumturquoise" => "#48d1cc",
    "mediumvioletred" => "#c71585",
    "midnightblue" => "#191970",
    "mintcream" => "#f5fffa",
    "mistyrose" => "#ffe4e1",
    "moccasin" => "#ffe4b5",
    "navajowhite" => "#ffdead",
    "navy" => "#000080",
    "oldlace" => "#fdf5e6",
    "olive" => "#808000",
    "olivedrab" => "#6b8e23",
    "orange" => "#ffa500",
    "orangered" => "#ff4500",
    "orchid" => "#da70d6",
    "palegoldenrod" => "#eee8aa",
    "palegreen" => "#98fb98",
    "paleturquoise" => "#afeeee",
    "palevioletred" => "#db7093",
    "papayawhip" => "#ffefd5",
    "peachpuff" => "#ffdab9",
    "peru" => "#cd853f",
    "pink" => "#ffc0cb",
    "plum" => "#dda0dd",
    "powderblue" => "#b0e0e6",
    "purple" => "#800080",
    "red" => "#ff0000",
    "rosybrown" => "#bc8f8f",
    "royalblue" => "#4169e1",
    "saddlebrown" => "#8b4513",
    "salmon" => "#fa8072",
    "sandybrown" => "#f4a460",
    "seagreen" => "#2e8b57",
    "seashell" => "#fff5ee",
    "sienna" => "#a0522d",
    "silver" => "#c0c0c0",
    "skyblue" => "#87ceeb",
    "slateblue" => "#6a5acd",
    "slategray" => "#708090",
    "slategrey" => "#708090",
    "snow" => "#fffafa",
    "springgreen" => "#00ff7f",
    "steelblue" => "#4682b4",
    "tan" => "#d2b48c",
    "teal" => "#008080",
    "thistle" => "#d8bfd8",
    "tomato" => "#ff6347",
    "turquoise" => "#40e0d0",
    "violet" => "#ee82ee",
    "wheat" => "#f5deb3",
    "white" => "#ffffff",
    "whitesmoke" => "#f5f5f5",
    "yellow" => "#ffff00",
    "yellowgreen" => "#9acd32",
};

fn css_color_hex_lowercase(name: &str) -> Option<&'static str> {
    COLOR_MAP.get(name).copied()
}

/// Look up a CSS named color (case-insensitive) and return its `#rrggbb` hex.
pub fn css_color_hex(name: &str) -> Option<&'static str> {
    let name = name.to_lowercase();
    css_color_hex_lowercase(&name)
}

/// Port of `convertHexToNumber`: drop the leading `#` and parse the rest as a
/// base-16 integer. Returns -1 for an unparseable hex (see the module note on
/// Strudel's `NaN`).
pub fn convert_hex_to_number(hex: &str) -> i64 {
    let digits = hex.strip_prefix('#').unwrap_or(hex);
    i64::from_str_radix(digits, 16).unwrap_or(-1)
}

/// Port of `convertColorToNumber`: lowercase the color, then return the number of
/// a `#hex` directly, of a named color via the table, or -1 if unrecognized.
pub fn convert_color_to_number(color: &str) -> i64 {
    let color = color.to_lowercase();

    if color.starts_with('#') {
        return convert_hex_to_number(&color);
    }

    match css_color_hex_lowercase(&color) {
        Some(hex) => convert_hex_to_number(hex),
        None => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_to_number_matches_strudel() {
        // parseInt('ff0000', 16) === 16711680
        assert_eq!(convert_hex_to_number("#ff0000"), 16711680);
        assert_eq!(convert_hex_to_number("#000000"), 0);
        assert_eq!(convert_hex_to_number("#ffffff"), 16777215);
        // works without a leading '#' too (slice is a no-op then)
        assert_eq!(convert_hex_to_number("00ff00"), 65280);
    }

    #[test]
    fn color_to_number_named_and_hex() {
        assert_eq!(convert_color_to_number("red"), 16711680);
        assert_eq!(convert_color_to_number("RED"), 16711680);
        assert_eq!(convert_color_to_number("#00ff00"), 65280);
        // named color resolves through the same hex path
        assert_eq!(convert_color_to_number("lime"), 65280);
    }

    #[test]
    fn color_to_number_unrecognized_is_minus_one() {
        assert_eq!(convert_color_to_number("notacolor"), -1);
        assert_eq!(convert_color_to_number(""), -1);
    }

    #[test]
    fn color_map_resolves_every_entry_through_conversion() {
        // Every named color must convert to the same number as its own hex.
        for (name, hex) in COLOR_MAP.entries() {
            assert_eq!(
                convert_color_to_number(name),
                convert_hex_to_number(hex),
                "color {name} should match its hex {hex}",
            );
            // and the number must be a valid 24-bit color (the table is #rrggbb)
            let n = convert_hex_to_number(hex);
            assert!((0..=0xff_ffff).contains(&n), "{name} out of 24-bit range");
        }
    }

    #[test]
    fn css_color_hex_is_case_insensitive() {
        assert_eq!(css_color_hex("CadetBlue"), Some("#5f9ea0"));
        assert_eq!(css_color_hex("cadetblue"), Some("#5f9ea0"));
        assert_eq!(css_color_hex("nope"), None);
    }
}
