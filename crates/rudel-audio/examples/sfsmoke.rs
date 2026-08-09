//! Manual smoke test: fetch a real General MIDI preset and render a note.
//! Not a unit test — it needs the network.
use rudel_audio::{OfflineMixer, SampleBank, collect_events, samples::decode_bytes, soundfont};

fn main() {
    for (name, note) in [("gm_piano", "c4"), ("gm_flute", "g5")] {
        let preset = soundfont::load_gm_preset(name, 0, fetch, decode_bytes).unwrap();
        println!("{name}: {} zones", preset.zones.len());
        let mut bank = SampleBank::new();
        bank.register_font(name, 0, preset);
        let pat = rudel_core::s(rudel_core::pure(rudel_core::Value::Str(name.into())))
            .note(rudel_core::Value::Str(note.into()));
        let mut mixer = OfflineMixer::new(44100.0);
        for ev in collect_events(&pat, 1.0, 0.0, 1.0, &bank) {
            mixer.schedule(ev);
        }
        let peak = (0..44100)
            .map(|_| {
                let (l, r) = mixer.render_frame();
                l.abs().max(r.abs())
            })
            .fold(0.0f32, f32::max);
        println!("  {note} peak {peak:.4}");
        assert!(peak > 0.001, "{name} should make sound");
    }
    println!("ok");
}

fn fetch(url: &str) -> Result<String, String> {
    let mut r = ureq::get(url).call().map_err(|e| e.to_string())?;
    r.body_mut().read_to_string().map_err(|e| e.to_string())
}
