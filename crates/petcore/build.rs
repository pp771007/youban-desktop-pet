//! 把 `design/tokens.json`（設計參數的唯一來源）轉成 Rust 常數。設定程式那邊讀同一份 JSON 產生 CSS 變數。

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use serde_json::{Map, Value};

const TOKENS: &str = "../../design/tokens.json";

const RESOURCES: &str = "app.rc";

fn main() {
    println!("cargo:rerun-if-changed={RESOURCES}");
    println!("cargo:rerun-if-changed=assets/app.ico");
    embed_resource::compile(RESOURCES, embed_resource::NONE).manifest_optional().expect("compile app.rc");
    println!("cargo:rerun-if-changed={TOKENS}");
    let json: Value = serde_json::from_str(&fs::read_to_string(TOKENS).expect("read tokens.json")).expect("parse tokens.json");
    let root = json.as_object().expect("tokens root is an object");
    let mut out = String::from("// 由 build.rs 從 design/tokens.json 產生，不要手改。\n\n");

    let colors = root["color"].as_object().expect("color");
    let light = colors["light"].as_object().expect("color.light");
    let dark = colors["dark"].as_object().expect("color.dark");
    let mut light_keys: Vec<_> = light.keys().collect();
    let mut dark_keys: Vec<_> = dark.keys().collect();
    light_keys.sort();
    dark_keys.sort();
    assert_eq!(light_keys, dark_keys, "color.light 與 color.dark 的欄位要一樣");

    out.push_str("#[derive(Debug, Clone, Copy)]\npub struct Palette {\n");
    for k in &light_keys {
        writeln!(out, "    pub {}: Rgba,", snake(k)).unwrap();
    }
    out.push_str("}\n\n");
    for (name, palette) in [("LIGHT", light), ("DARK", dark)] {
        writeln!(out, "pub const {name}: Palette = Palette {{").unwrap();
        for k in &light_keys {
            let (r, g, b) = hex(palette[*k].as_str().expect("color is a string"));
            writeln!(out, "    {}: Rgba {{ r: {r:.4}, g: {g:.4}, b: {b:.4}, a: 1.0 }},", snake(k)).unwrap();
        }
        out.push_str("};\n\n");
    }

    for (section, value) in root {
        if section != "color" {
            flatten(&mut out, &section.to_uppercase(), value);
        }
    }

    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("tokens.rs");
    fs::write(dest, out).unwrap();
}

fn flatten(out: &mut String, prefix: &str, value: &Value) {
    match value {
        Value::Object(map) => flatten_map(out, prefix, map),
        Value::Number(n) => writeln!(out, "pub const {prefix}: f32 = {:?};", n.as_f64().unwrap() as f32).unwrap(),
        Value::String(s) => writeln!(out, "pub const {prefix}: &str = {s:?};").unwrap(),
        Value::Array(items) => {
            let nums: Vec<String> = items.iter().map(|v| format!("{:?}", v.as_f64().expect("number array") as f32)).collect();
            writeln!(out, "pub const {prefix}: [f32; {}] = [{}];", nums.len(), nums.join(", ")).unwrap();
        }
        _ => panic!("unsupported token at {prefix}"),
    }
}

fn flatten_map(out: &mut String, prefix: &str, map: &Map<String, Value>) {
    for (k, v) in map {
        flatten(out, &format!("{prefix}_{}", snake(k).to_uppercase()), v);
    }
}

fn snake(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn hex(s: &str) -> (f32, f32, f32) {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).expect("hex color");
    let c = |shift: u32| ((v >> shift) & 0xFF) as f32 / 255.0;
    (c(16), c(8), c(0))
}
