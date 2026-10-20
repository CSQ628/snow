#Requires -Version 7.0
# Offscreen native windows only; no installer or interaction with other processes.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'snow-shot-winget-test-windows.ps1')
if (-not ('SnowShotWingetWindowFixture' -as [type])) {
    Add-Type @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using System.Diagnostics;
public static class SnowShotWingetWindowFixture {
    private delegate IntPtr DialogCallback(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    private static readonly DialogCallback dialogCallback = OnDialogMessage;
    public static bool RunInvoked;
    private static IntPtr runButton;
    [StructLayout(LayoutKind.Sequential)] private struct Message {
        public IntPtr window;
        public uint message;
        public UIntPtr wParam;
        public IntPtr lParam;
        public uint time;
        public int x;
        public int y;
        public uint privateData;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateWindowEx(uint extendedStyle, string className, string name,
        uint style, int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr data);
    [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern bool EnableWindow(IntPtr window, bool enabled);
    public static void SetRunEnabled(bool enabled) { EnableWindow(runButton, enabled); }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr CreateDialogIndirectParam(IntPtr instance, IntPtr template, IntPtr owner,
        DialogCallback callback, IntPtr data);
    [DllImport("user32.dll")] private static extern bool SetWindowPos(IntPtr window, IntPtr after,
        int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] private static extern bool PeekMessage(out Message message, IntPtr window,
        uint minimum, uint maximum, uint flags);
    [DllImport("user32.dll")] private static extern IntPtr DispatchMessage(ref Message message);
    private static IntPtr OnDialogMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam) {
        if (message == 0x0111 && wParam.ToInt64() == 42 && lParam == runButton) {
            RunInvoked = true;
            return new IntPtr(1);
        }
        return IntPtr.Zero;
    }
    public static IntPtr CreateConsentDialog(string fileName, IntPtr owner) {
        var title = Encoding.Unicode.GetBytes("Open File - Security Warning\0");
        var template = new byte[22 + title.Length];
        BitConverter.GetBytes(0x80000000U).CopyTo(template, 0);
        BitConverter.GetBytes((short)100).CopyTo(template, 14);
        BitConverter.GetBytes((short)100).CopyTo(template, 16);
        title.CopyTo(template, 22);
        var pinned = GCHandle.Alloc(template, GCHandleType.Pinned);
        IntPtr dialog;
        try { dialog = CreateDialogIndirectParam(IntPtr.Zero, pinned.AddrOfPinnedObject(), owner, dialogCallback, IntPtr.Zero); }
        finally { pinned.Free(); }
        if (dialog == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
        try {
            var label = CreateWindowEx(0, "STATIC", fileName, 0x50000000U, 0, 0, 100, 20,
                dialog, new IntPtr(43), IntPtr.Zero, IntPtr.Zero);
            runButton = CreateWindowEx(0, "BUTTON", "&Run", 0x50000000U, 0, 20, 100, 20,
                dialog, new IntPtr(42), IntPtr.Zero, IntPtr.Zero);
            if (label == IntPtr.Zero || runButton == IntPtr.Zero ||
                !SetWindowPos(dialog, new IntPtr(1), -32000, -32000, 100, 100, 0x0050))
                throw new Win32Exception(Marshal.GetLastWin32Error());
            RunInvoked = false;
            return dialog;
        } catch { DestroyWindow(dialog); throw; }
    }
    public static bool WaitForRun() {
        var timer = Stopwatch.StartNew();
        while (!RunInvoked && timer.ElapsedMilliseconds < 2000) {
            Message message;
            if (PeekMessage(out message, IntPtr.Zero, 0, 0, 1)) DispatchMessage(ref message);
        }
        return RunInvoked;
    }
    public static IntPtr Create(string title, IntPtr owner, bool visible) {
        var window = CreateWindowEx(0, "STATIC", title, 0x80000000U | (visible ? 0x10000000U : 0),
            -32000, -32000, 100, 100, owner, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
        if (window == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
        return window;
    }
}
'@
}
$windows = [Collections.Generic.List[IntPtr]]::new()
try {
    $container = [SnowShotWingetWindowFixture]::Create('Window Dialog', [IntPtr]::Zero, $true)
    $windows.Add($container)
    $warning = [SnowShotWingetWindowFixture]::Create('Open File - Security Warning', $container, $true)
    $windows.Add($warning)
    $hidden = [SnowShotWingetWindowFixture]::Create('Hidden launch helper', [IntPtr]::Zero, $false)
    $windows.Add($hidden)
    $process = [Diagnostics.Process]::GetCurrentProcess()
    $process.Refresh()
    if ($process.MainWindowHandle -eq $warning) {
        throw 'The fixture must reproduce a consent dialog excluded by the main window lookup.'
    }
    $visible = @([WinGetWindowDiagnostics]::ForProcess($PID))
    if ($container -notin $visible -or $warning -notin $visible -or $hidden -in $visible) {
        throw 'WinGet window enumeration must include visible owned launch warnings and exclude hidden windows.'
    }
    if ([WinGetWindowDiagnostics]::Title($warning) -cne 'Open File - Security Warning' -or
        [WinGetWindowDiagnostics]::Read($warning) -cne 'Open File - Security Warning') {
        throw 'Owned launch warning text must remain available for verified installer consent.'
    }
    if (@([WinGetWindowDiagnostics]::ForProcess(-1)).Count) {
        throw 'Unrelated processes must not supply installer consent windows.'
    }
    $fileName = 'snow-shot-fixture-windows-arm64-offline.exe'
    $consent = [SnowShotWingetWindowFixture]::CreateConsentDialog($fileName, $container)
    $windows.Add($consent)
    $rejected = $false
    try { [WinGetWindowDiagnostics]::ConfirmVerifiedFile($consent, 'different-installer.exe') }
    catch { $rejected = $_.Exception.ToString().Contains('does not identify the verified installer') }
    if (-not $rejected -or [SnowShotWingetWindowFixture]::RunInvoked) {
        throw 'An unrelated installer must never receive consent.'
    }
    [SnowShotWingetWindowFixture]::SetRunEnabled($false)
    if ([WinGetWindowDiagnostics]::ConfirmVerifiedFile($consent, $fileName) -or
        [SnowShotWingetWindowFixture]::RunInvoked) {
        throw 'A disabled Run button must keep waiting without claiming consent was sent.'
    }
    [SnowShotWingetWindowFixture]::SetRunEnabled($true)
    if (-not [WinGetWindowDiagnostics]::ConfirmVerifiedFile($consent, $fileName)) {
        throw 'The enabled Run button must accept verified consent.'
    }
    if (-not [SnowShotWingetWindowFixture]::WaitForRun()) {
        throw 'Verified consent must reach an inactive owned dialog without foreground input.'
    }
    Write-Output 'PASS: WinGet owned launch warning discovery and inactive dialog consent with offscreen native windows.'
} finally {
    for ($index = $windows.Count - 1; $index -ge 0; $index--) {
        $null = [SnowShotWingetWindowFixture]::DestroyWindow($windows[$index])
    }
}
