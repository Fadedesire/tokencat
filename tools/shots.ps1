# Снимки окон TokenCat для витрины (B7). Снимает ТОЛЬКО прямоугольники окон tokencat.exe
# (панель, виджет в панели задач, настройки), а не весь экран — чужие окна не попадают.
# Результат: marketing\raw\<время>-<n>-<ширина>x<высота>.png
param([int]$Delay = 6, [int]$Pad = 16)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class W {
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr p);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc f, IntPtr p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out int v, int s);
}
"@
[W]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null   # физические пиксели

Start-Sleep -Seconds $Delay

$pids = @(Get-Process -Name tokencat -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
$root = Split-Path -Parent $PSScriptRoot
$out = Join-Path $root "marketing\raw"
New-Item -ItemType Directory -Force -Path $out | Out-Null
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$script:n = 0
$log = @()

$cb = [W+EnumProc]{
  param($h, $p)
  $procId = 0; [W]::GetWindowThreadProcessId($h, [ref]$procId) | Out-Null
  if ($pids -notcontains [int]$procId) { return $true }
  if (-not [W]::IsWindowVisible($h)) { return $true }
  $cloaked = 0; [W]::DwmGetWindowAttribute($h, 14, [ref]$cloaked, 4) | Out-Null
  if ($cloaked -ne 0) { return $true }
  $r = New-Object W+RECT
  [W]::DwmGetWindowAttribute($h, 9, [ref]$r, 16) | Out-Null   # видимые границы без тени
  $w = $r.R - $r.L; $hh = $r.B - $r.T
  if ($w -lt 20 -or $hh -lt 20) { return $true }
  $px = $Pad; $py = $Pad
  if ($hh -lt 90) { $px = 220; $py = 6 }   # виджет в панели задач — с кусочком панели вокруг
  $x = $r.L - $px; $y = $r.T - $py; $cw = $w + 2*$px; $ch = $hh + 2*$py
  $bmp = New-Object System.Drawing.Bitmap $cw, $ch
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
  $script:n++
  $file = Join-Path $out ("{0}-{1}-{2}x{3}.png" -f $stamp, $script:n, $w, $hh)
  $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  $script:log += "$file  rect=$($r.L),$($r.T),$w,$hh"
  return $true
}
[W]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null

# Виджет в панели задач — дочернее окно панели задач, EnumWindows его не видит.
$cbChild = [W+EnumProc]{
  param($h, $p)
  $procId = 0; [W]::GetWindowThreadProcessId($h, [ref]$procId) | Out-Null
  if ($pids -notcontains [int]$procId) { return $true }
  if (-not [W]::IsWindowVisible($h)) { return $true }
  $r = New-Object W+RECT
  [W]::GetWindowRect($h, [ref]$r) | Out-Null
  $w = $r.R - $r.L; $hh = $r.B - $r.T
  if ($w -lt 20 -or $hh -lt 20) { return $true }
  $x = $r.L - 260; $y = $r.T; $cw = $w + 520; $ch = $hh
  $bmp = New-Object System.Drawing.Bitmap $cw, $ch
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
  $script:n++
  $file = Join-Path $out ("{0}-{1}-taskbar-{2}x{3}.png" -f $stamp, $script:n, $w, $hh)
  $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  $script:log += "$file  child rect=$($r.L),$($r.T),$w,$hh"
  return $true
}
foreach ($cls in @("Shell_TrayWnd")) {
  $tb = [W]::FindWindow($cls, $null)
  if ($tb -ne [IntPtr]::Zero) { [W]::EnumChildWindows($tb, $cbChild, [IntPtr]::Zero) | Out-Null }
}
$script:log | Set-Content -Encoding UTF8 (Join-Path $root "logs\shots.log")
