//! Regenerates crates/rudel-core/src/tune_table.rs from Strudel's tunejs.js.
//! Run from the repo root: zig run tools/generate_tune_table.zig
const std = @import("std");
const Io = std.Io;

const src_path = "strudel/packages/xen/tunejs.js";
const out_path = "crates/rudel-core/src/tune_table.rs";

const header =
    \\// Generated from strudel/packages/xen/tunejs.js. Do not edit by hand.
    \\// Strudel/Tune.js scale names are case-sensitive.
    \\//
    \\// tune.js stores each scale as absolute frequencies whose final entry is the
    \\// octave duplicate of the tonic. Rudel only consumes octave-normalised ratios,
    \\// so they are pre-divided here (each freq / the tonic, dropping the octave) at
    \\// generation time. Named-scale lookups therefore return a static ratio slice
    \\// directly, with no per-call division or allocation.
    \\
    \\#[rustfmt::skip]
    \\pub(crate) static TUNE_SCALES: phf::Map<&'static str, &'static [f64]> = phf::phf_map! {
    \\
;

fn lessThan(_: void, a: []const u8, b: []const u8) bool {
    return std.mem.lessThan(u8, a, b);
}

pub fn main(init: std.process.Init) !void {
    const gpa = init.arena.allocator(); // short-lived tool: never freed
    const io = init.io;
    const cwd = Io.Dir.cwd();

    const text = try cwd.readFileAlloc(io, src_path, gpa, .limited(64 << 20));
    const marker = "var TuningList = ";
    const start = (std.mem.indexOf(u8, text, marker) orelse return error.TuningListNotFound) + marker.len;
    const end = std.mem.lastIndexOfScalar(u8, text, '}') orelse return error.TuningListNotFound;
    const parsed = try std.json.parseFromSlice(std.json.Value, gpa, text[start .. end + 1], .{});
    const archive = parsed.value.object;

    const names = try gpa.alloc([]const u8, archive.count());
    @memcpy(names, archive.keys());
    std.mem.sort([]const u8, names, {}, lessThan);

    var out: Io.Writer.Allocating = .init(gpa);
    const w = &out.writer;
    try w.writeAll(header);
    var count: usize = 0;
    for (names) |name| {
        const freqs_value = archive.get(name).?.object.get("frequencies") orelse continue;
        const freqs = freqs_value.array.items;
        if (freqs.len == 0) continue;
        const tonic = try number(freqs[0]);
        if (tonic == 0.0) continue;
        try w.print("    {f} => &[", .{std.json.fmt(name, .{})});
        // Drop the trailing octave duplicate and normalise to ratios: the same
        // IEEE-754 division the runtime used, printed as the shortest decimal
        // that round-trips to the identical double.
        const n = freqs.len - 1;
        if (n == 0) try ratio(w, 1.0);
        for (freqs[0..n], 0..) |f, i| {
            if (i != 0) try w.writeAll(", ");
            try ratio(w, try number(f) / tonic);
        }
        try w.writeAll("],\n");
        count += 1;
    }
    try w.writeAll("};\n");

    try cwd.writeFile(io, .{ .sub_path = out_path, .data = out.written() });
    std.debug.print("wrote {s} with {d} scales\n", .{ out_path, count });
}

fn number(v: std.json.Value) !f64 {
    return switch (v) {
        .float => |f| f,
        .integer => |i| @floatFromInt(i),
        .number_string => |s| try std.fmt.parseFloat(f64, s),
        else => error.NotANumber,
    };
}

/// Rust rejects `1` as an f64 literal, so keep the `.0` Python's repr gave.
fn ratio(w: *Io.Writer, r: f64) !void {
    var buf: [64]u8 = undefined;
    const s = try std.fmt.bufPrint(&buf, "{d}", .{r});
    try w.writeAll(s);
    if (std.mem.indexOfScalar(u8, s, '.') == null) try w.writeAll(".0");
}
