//! Build Rudel's pinned static FFmpeg libraries (once, before cargo build).
//! Run from the repo root: zig run tools/build-ffmpeg.zig
const std = @import("std");
const builtin = @import("builtin");
const Io = std.Io;

const script = @embedFile("build-ffmpeg.zig");
const windows = builtin.os.tag == .windows;
const vcpkg = "target/ffmpeg-vcpkg";
const archive = "target/ffmpeg-sources.tar.gz";

const triplet = switch (builtin.os.tag) {
    .windows => "x64-windows-static-md",
    .linux => if (builtin.cpu.arch == .aarch64) "arm64-linux" else "x64-linux",
    .macos => if (builtin.cpu.arch == .aarch64) "arm64-osx" else "x64-osx",
    else => @compileError("no vcpkg triplet for this host"),
};

var io: Io = undefined;
var gpa: std.mem.Allocator = undefined; // short-lived tool: the arena is never freed

fn exists(path: []const u8) bool {
    Io.Dir.cwd().access(io, path, .{}) catch return false;
    return true;
}

fn spawn(argv: []const []const u8, cwd: ?[]const u8, quiet: bool) !std.process.Child.Term {
    var child = try std.process.spawn(io, .{
        .argv = argv,
        .cwd = if (cwd) |c| .{ .path = c } else .inherit,
        .stdout = if (quiet) .ignore else .inherit,
        .stderr = if (quiet) .ignore else .inherit,
    });
    return child.wait(io);
}

fn run(argv: []const []const u8, cwd: ?[]const u8) !void {
    if (!(try spawn(argv, cwd, false)).success()) return error.CommandFailed;
}

fn which(env: *const std.process.Environ.Map, name: []const u8) !?[]const u8 {
    var dirs = std.mem.tokenizeScalar(u8, env.get("PATH") orelse return null, std.fs.path.delimiter);
    while (dirs.next()) |dir| {
        const path = try std.fs.path.join(gpa, &.{ dir, if (windows) try std.mem.concat(gpa, u8, &.{ name, ".exe" }) else name });
        if (exists(path)) return path;
    }
    return null;
}

fn posix(path: []const u8) ![]u8 {
    const out = try gpa.dupe(u8, path);
    std.mem.replaceScalar(u8, out, '\\', '/');
    return out;
}

pub fn main(init: std.process.Init) !void {
    io = init.io;
    gpa = init.arena.allocator();
    const cwd = Io.Dir.cwd();

    const vcpkg_json = try cwd.readFileAlloc(io, "vcpkg.json", gpa, .limited(1 << 20));
    const parsed = try std.json.parseFromSlice(std.json.Value, gpa, vcpkg_json, .{});
    const revision = parsed.value.object.get("builtin-baseline").?.string;

    if (!exists(vcpkg ++ "/.git")) {
        try run(&.{ "git", "clone", "--no-checkout", "--filter=blob:none", "https://github.com/microsoft/vcpkg.git", vcpkg }, null);
    }
    if ((try spawn(&.{ "git", "cat-file", "-e", revision }, vcpkg, true)).success() == false) {
        try run(&.{ "git", "fetch", "--depth=1", "origin", revision }, vcpkg);
    }
    try run(&.{ "git", "checkout", revision }, vcpkg);

    const executable = vcpkg ++ (if (windows) "/vcpkg.exe" else "/vcpkg");
    if (!exists(executable)) {
        if (windows) {
            try run(&.{ "target\\ffmpeg-vcpkg\\bootstrap-vcpkg.bat", "-disableMetrics" }, null);
        } else {
            try run(&.{ "sh", vcpkg ++ "/bootstrap-vcpkg.sh", "-disableMetrics" }, null);
        }
    }

    // Rust's dev/release profiles can both link these optimised C libraries.
    // Keep the standard static triplet, avoiding a second, unused debug build.
    const overlay = "target/ffmpeg-triplets";
    try cwd.createDirPath(io, overlay);
    const root = try posix(try cwd.realPathFileAlloc(io, ".", gpa));
    const triplet_file = triplet ++ ".cmake";
    const source = if (exists(vcpkg ++ "/triplets/" ++ triplet_file))
        vcpkg ++ "/triplets/" ++ triplet_file
    else
        vcpkg ++ "/triplets/community/" ++ triplet_file;

    var settings: Io.Writer.Allocating = .init(gpa);
    const w = &settings.writer;
    try w.writeAll(try cwd.readFileAlloc(io, source, gpa, .limited(1 << 20)));
    try w.writeAll("\nset(VCPKG_BUILD_TYPE release)\n");
    if (windows) {
        // clang-cl uses the MSVC ABI/SDK, and avoids older MSVC's pathological
        // optimisation time on FFmpeg's large legacy codec functions.
        const fallback = "C:/Program Files/LLVM/bin/clang-cl.exe";
        const clang = try posix((try which(init.environ_map, "clang-cl")) orelse
            if (exists(fallback)) fallback else {
                std.debug.print("Install LLVM (clang-cl and libclang) before building FFmpeg\n", .{});
                return error.MissingLlvm;
            });
        try cwd.writeFile(io, .{
            .sub_path = overlay ++ "/clang.cmake",
            .data = try std.fmt.allocPrint(gpa,
                \\set(CMAKE_C_COMPILER "{[clang]s}")
                \\set(CMAKE_CXX_COMPILER "{[clang]s}")
                \\include("{[root]s}/{[vcpkg]s}/scripts/toolchains/windows.cmake")
                \\
            , .{ .clang = clang, .root = root, .vcpkg = vcpkg }),
        });
        // Only FFmpeg itself: meson drives clang-cl as a linker and hands it
        // bare /LIBPATH: options, so dav1d stays on MSVC's own toolchain.
        try w.print(
            \\set(VCPKG_LOAD_VCVARS_ENV ON)
            \\if(PORT STREQUAL "ffmpeg")
            \\  set(VCPKG_CHAINLOAD_TOOLCHAIN_FILE "{[root]s}/{[overlay]s}/clang.cmake")
            \\  set(ENV{{PATH}} "{[bin]s};$ENV{{PATH}}")
            \\endif()
            \\
        , .{ .root = root, .overlay = overlay, .bin = std.fs.path.dirnamePosix(clang) orelse "." });
    } else if (try which(init.environ_map, "nasm") == null) {
        // vcpkg downloads its own only on Windows; dav1d's assembly needs it.
        std.debug.print("Install nasm before building FFmpeg (apt install nasm / brew install nasm)\n", .{});
        return error.MissingNasm;
    }
    try cwd.writeFile(io, .{ .sub_path = overlay ++ "/" ++ triplet_file, .data = settings.written() });

    try run(&.{
        executable,
        "install",
        "--triplet=" ++ triplet,
        "--host-triplet=" ++ triplet,
        "--overlay-triplets=" ++ overlay,
        "--x-install-root=target/ffmpeg",
        "--disable-metrics",
        "--binarysource=clear",
    }, null);

    // Cache this with the installed libraries. Releases offer the actual
    // patched native sources beside the binary, not just an upstream URL.
    if (archiveCurrent(vcpkg_json)) return;
    const temporary = "target/ffmpeg-sources.tar.tmp";
    {
        var file = try cwd.createFile(io, temporary, .{});
        defer file.close(io);
        var file_buffer: [16384]u8 = undefined;
        var file_writer = file.writer(io, &file_buffer);
        var window: [std.compress.flate.max_window_len]u8 = undefined;
        var gzip = try std.compress.flate.Compress.init(&file_writer.interface, &window, .gzip, .default);
        var tar: std.tar.Writer = .{ .underlying_writer = &gzip.writer };
        // The two small files go first so `archiveCurrent` can stop reading early.
        try tar.writeFileBytes("vcpkg.json", vcpkg_json, .{});
        try tar.writeFileBytes("tools/build-ffmpeg.zig", script, .{});
        for ([_][]const u8{ "ffmpeg", "dav1d", "zlib", "openssl" }) |port| {
            if (windows and std.mem.eql(u8, port, "openssl")) continue;
            const src = try std.fmt.allocPrint(gpa, vcpkg ++ "/buildtrees/{s}/src", .{port});
            if (!exists(src)) {
                std.debug.print("Missing {s} source; rebuild with --binarysource=clear\n", .{port});
                return error.MissingSource;
            }
            try addTree(&tar, src, port);
            try addTree(&tar, try std.fmt.allocPrint(gpa, vcpkg ++ "/ports/{s}", .{port}), try std.fmt.allocPrint(gpa, "vcpkg-ports/{s}", .{port}));
        }
        try gzip.finish();
        try file_writer.interface.flush();
    }
    try Io.Dir.rename(cwd, temporary, cwd, archive, io);
}

fn addTree(tar: *std.tar.Writer, src: []const u8, arc: []const u8) !void {
    var dir = try Io.Dir.cwd().openDir(io, src, .{ .iterate = true });
    defer dir.close(io);
    try tar.writeDir(arc, .{});
    var walker = try dir.walk(gpa);
    defer walker.deinit();
    while (try walker.next(io)) |entry| {
        const name = try posix(try std.fmt.allocPrint(gpa, "{s}/{s}", .{ arc, entry.path }));
        switch (entry.kind) {
            .directory => try tar.writeDir(name, .{}),
            .file => {
                var file = try entry.dir.openFile(io, entry.basename, .{});
                defer file.close(io);
                var buffer: [16384]u8 = undefined;
                var reader = file.reader(io, &buffer);
                try tar.writeFile(name, &reader, 0);
            },
            .sym_link => {
                var target: [4096]u8 = undefined;
                const len = try entry.dir.readLink(io, entry.basename, &target);
                try tar.writeLink(name, target[0..len], .{});
            },
            else => {},
        }
    }
}

/// True when the archive was built from this vcpkg.json and this script.
fn archiveCurrent(vcpkg_json: []const u8) bool {
    return archiveMatches(vcpkg_json) catch false;
}

fn archiveMatches(vcpkg_json: []const u8) !bool {
    var file = try Io.Dir.cwd().openFile(io, archive, .{});
    defer file.close(io);
    var file_buffer: [16384]u8 = undefined;
    var file_reader = file.reader(io, &file_buffer);
    var window: [std.compress.flate.max_window_len]u8 = undefined;
    var gunzip = std.compress.flate.Decompress.init(&file_reader.interface, .gzip, &window);
    var names: [std.fs.max_path_bytes]u8 = undefined;
    var links: [std.fs.max_path_bytes]u8 = undefined;
    var it = std.tar.Iterator.init(&gunzip.reader, .{ .file_name_buffer = &names, .link_name_buffer = &links });
    inline for (.{ .{ "vcpkg.json", vcpkg_json }, .{ "tools/build-ffmpeg.zig", script } }) |want| {
        const entry = (try it.next()) orelse return false;
        if (!std.mem.eql(u8, entry.name, want[0])) return false;
        var content: Io.Writer.Allocating = .init(gpa);
        try it.streamRemaining(entry, &content.writer);
        if (!std.mem.eql(u8, content.written(), want[1])) return false;
    }
    return true;
}
