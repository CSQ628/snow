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
public static class SnowShotWingetWindowFixture {
    private delegate IntPtr DialogCallback(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    private static readonly DialogCallback dialogCallback = OnDialogMessage;
    private static IntPtr runButton;
    public static IntPtr RunButton { get { return runButton; } }
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateWindowEx(uint extendedStyle, string className, string name,
        uint style, int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr data);
    [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern bool EnableWindow(IntPtr window, bool enabled);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr window);
    public static void SetRunEnabled(bool enabled) { EnableWindow(runButton, enabled); }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr CreateDialogIndirectParam(IntPtr instance, IntPtr template, IntPtr owner,
        DialogCallback callback, IntPtr data);
    [DllImport("user32.dll")] private static extern bool SetWindowPos(IntPtr window, IntPtr after,
        int x, int y, int width, int height, uint flags);
    private static IntPtr OnDialogMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam) {
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
            return dialog;
        } catch { DestroyWindow(dialog); throw; }
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
$foreground = [SnowShotWingetWindowFixture]::GetForegroundWindow()
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
    # Foreground permission is OS-owned state. Exercise allowed and denied
    # outcomes deterministically without requesting focus for offscreen windows.
    $script:activationAllowed = $false
    $script:activationCalls = 0
    $script:clicks = 0
    $activate = [Func[IntPtr, bool]]{
        param($window)
        if ($window -ne $consent) { throw 'Activation targeted an unrelated window.' }
        $script:activationCalls++
        return $script:activationAllowed
    }
    $enabled = [Func[IntPtr, bool]]{
        param($button)
        return [SnowShotWingetWindowFixture]::IsWindowEnabled($button)
    }
    $click = [Func[IntPtr, bool]]{
        param($button)
        if ($button -ne [SnowShotWingetWindowFixture]::RunButton) { throw 'Consent targeted an unrelated button.' }
        $script:clicks++
        return $true
    }
    $rejected = $false
    try { [WinGetWindowDiagnostics]::ConfirmVerifiedFile($consent, 'different-installer.exe', $activate, $enabled, $click) }
    catch { $rejected = $_.Exception.ToString().Contains('does not identify the verified installer') }
    if (-not $rejected -or $script:activationCalls -ne 0 -or $script:clicks -ne 0) {
        throw 'An unrelated installer must never receive consent.'
    }
    if ([WinGetWindowDiagnostics]::ConfirmVerifiedFile($consent, $fileName, $activate, $enabled, $click) -or $script:clicks -ne 0) {
        throw 'Denied foreground activation must keep waiting without clicking Run.'
    }
    $script:activationAllowed = $true
    [SnowShotWingetWindowFixture]::SetRunEnabled($false)
    if ([WinGetWindowDiagnostics]::ConfirmVerifiedFile($consent, $fileName, $activate, $enabled, $click) -or $script:clicks -ne 0) {
        throw 'A disabled Run button must keep waiting without claiming consent was sent.'
    }
    [SnowShotWingetWindowFixture]::SetRunEnabled($true)
    if (-not [WinGetWindowDiagnostics]::ConfirmVerifiedFile($consent, $fileName, $activate, $enabled, $click)) {
        throw 'The enabled Run button must accept verified consent.'
    }
    if ($script:clicks -ne 1) {
        throw 'Verified consent must invoke exactly the identified Run button.'
    }
    Write-Output 'PASS: WinGet owned launch warning discovery and inactive dialog consent with offscreen native windows.'
} finally {
    if ($foreground -ne [IntPtr]::Zero) {
        $null = [SnowShotWingetWindowFixture]::SetForegroundWindow($foreground)
    }
    for ($index = $windows.Count - 1; $index -ge 0; $index--) {
        $null = [SnowShotWingetWindowFixture]::DestroyWindow($windows[$index])
    }
}
