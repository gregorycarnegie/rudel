<#
.SYNOPSIS
Drive the rudel egui app: launch it, paste code in, evaluate, screenshot.

.DESCRIPTION
rudel is a native winit/egui window, so there is no DOM, no accessibility tree
and no CLI flag that loads a script. The only way in is the OS: put text on the
clipboard, paste it into the editor, and click the toolbar with synthetic mouse
input. Everything here is doing that, DPI-correctly.

Actions run in the order of the switches below, so one invocation can do a whole
flow:

  driver.ps1 -Launch -Play -Eval 's("bd*4").spiral()' -Wait 3 -Shot out.png

.NOTES
Run from the repo root. Requires target/release/rudel.exe (cargo build --release
-p rudel-app). Needs a real interactive desktop session — this drives the actual
cursor, so don't use the machine while it runs.
#>
[CmdletBinding()]
param(
    # Start the app (no-op if already running) and wait for its window.
    [switch]$Launch,
    # Replace the editor contents with $Eval and evaluate it.
    [string]$Eval,
    # Click transport Play / Stop.
    [switch]$Play,
    [switch]$Stop,
    # Save a PNG of the whole window.
    [string]$Shot,
    # Seconds to sleep before the screenshot, to let the pattern advance.
    [double]$Wait = 0,
    # Close the app.
    [switch]$Quit,
    # Override the auto-detected DPI scale (1.0 = 96 dpi).
    [double]$Scale = 0
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class RudelWin {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, int e);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

# Without this the screen coordinates PowerShell sees are virtualized on a
# scaled display, and CopyFromScreen grabs the wrong region entirely (typically
# the top-left of the desktop instead of the window).
[RudelWin]::SetProcessDPIAware() | Out-Null

# Toolbar buttons in egui *logical* points, measured from the window's top-left.
# Scaled by the window's DPI at click time.
$Buttons = @{
    Play   = @(125, 47)
    Eval   = @(190, 47)
    Editor = @(200, 150)
}

function Get-RudelWindow {
    $proc = Get-Process rudel -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if (-not $proc) { throw "rudel is not running (use -Launch)" }
    $r = New-Object RudelWin+RECT
    [RudelWin]::GetWindowRect($proc.MainWindowHandle, [ref]$r) | Out-Null
    $dpi = [RudelWin]::GetDpiForWindow($proc.MainWindowHandle)
    $s = if ($Scale -gt 0) { $Scale } elseif ($dpi -gt 0) { $dpi / 96.0 } else { 1.0 }
    [pscustomobject]@{
        Handle = $proc.MainWindowHandle
        Left   = $r.Left; Top = $r.Top
        Width  = $r.Right - $r.Left; Height = $r.Bottom - $r.Top
        Scale  = $s
    }
}

function Focus-Rudel {
    $w = Get-RudelWindow
    [RudelWin]::SetForegroundWindow($w.Handle) | Out-Null
    Start-Sleep -Milliseconds 400
    $w
}

# Click a point given in egui logical points from the window's top-left.
function Invoke-RudelClick([double]$X, [double]$Y) {
    $w = Focus-Rudel
    $px = [int]($w.Left + $X * $w.Scale)
    $py = [int]($w.Top + $Y * $w.Scale)
    [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point($px, $py)
    Start-Sleep -Milliseconds 150
    [RudelWin]::mouse_event(0x02, 0, 0, 0, 0)   # left down
    [RudelWin]::mouse_event(0x04, 0, 0, 0, 0)   # left up
    Start-Sleep -Milliseconds 350
}

function Invoke-RudelButton([string]$Name) {
    $xy = $Buttons[$Name]
    if (-not $xy) { throw "unknown button '$Name'" }
    Invoke-RudelClick $xy[0] $xy[1]
}

if ($Launch) {
    $exe = Join-Path (Get-Location) 'target\release\rudel.exe'
    if (-not (Test-Path $exe)) { throw "missing $exe - run: cargo build --release -p rudel-app" }
    if (-not (Get-Process rudel -ErrorAction SilentlyContinue)) {
        Start-Process -FilePath $exe -WorkingDirectory (Get-Location)
    }
    # winit publishes the window handle before the window is laid out, so it
    # reports a ~22x22 rect for the first moment. Clicking then lands nowhere.
    # Wait for a plausible size, not just a handle.
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        $ready = $false
        try { $ready = (Get-RudelWindow).Width -gt 400 } catch { $ready = $false }
        if ($ready) { break }
        Start-Sleep -Milliseconds 300
    }
    Start-Sleep -Milliseconds 500
    $w = Get-RudelWindow
    "launched $($w.Width)x$($w.Height) at $($w.Left),$($w.Top) scale $($w.Scale)"
}

if ($Play) { Invoke-RudelButton Play; "play" }
if ($Stop) { Invoke-RudelButton Play; "stop" }   # same button toggles

if ($PSBoundParameters.ContainsKey('Eval')) {
    # Paste rather than type: the editor auto-pairs brackets and quotes, so
    # SendKeys-ing source text produces mangled code.
    Set-Clipboard -Value $Eval
    Invoke-RudelButton Editor          # focus the TextEdit
    $sh = New-Object -ComObject WScript.Shell
    $sh.SendKeys('^a'); Start-Sleep -Milliseconds 250
    $sh.SendKeys('^v'); Start-Sleep -Milliseconds 500
    # Ctrl+Enter is the documented eval shortcut but synthetic SendKeys does not
    # reach the egui window - click the Eval button instead (see SKILL.md).
    Invoke-RudelButton Eval
    Start-Sleep -Milliseconds 600
    "evaluated"
}

if ($Wait -gt 0) { Start-Sleep -Seconds $Wait }

if ($Shot) {
    $w = Focus-Rudel
    $bmp = New-Object System.Drawing.Bitmap($w.Width, $w.Height)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($w.Left, $w.Top, 0, 0, (New-Object System.Drawing.Size($w.Width, $w.Height)))
    $dir = Split-Path -Parent $Shot
    if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
    $bmp.Save($Shot, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    "saved $Shot ($($w.Width)x$($w.Height))"
}

if ($Quit) {
    Get-Process rudel -ErrorAction SilentlyContinue | Stop-Process -Force
    "quit"
}
