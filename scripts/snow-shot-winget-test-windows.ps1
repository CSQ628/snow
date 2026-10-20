#Requires -Version 7.0
if ('WinGetWindowDiagnostics' -as [type]) { return }
Add-Type @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class WinGetWindowDiagnostics {
    private delegate bool Callback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] private static extern bool EnumWindows(Callback callback, IntPtr data);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
    [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
    [DllImport("user32.dll", SetLastError = true)] private static extern bool AttachThreadInput(uint thread, uint other, bool attach);
    [DllImport("user32.dll")] private static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] private static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern bool BringWindowToTop(IntPtr window);
    [DllImport("user32.dll")] private static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll")] private static extern bool EnumChildWindows(IntPtr window, Callback callback, IntPtr data);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")] private static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    public static IntPtr[] ForProcess(int processId) {
        var windows = new List<IntPtr>();
        EnumWindows((window, data) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner == processId && IsWindowVisible(window)) windows.Add(window);
            return true;
        }, IntPtr.Zero);
        return windows.ToArray();
    }
    public static string Title(IntPtr window) {
        var text = new StringBuilder(2048);
        GetWindowText(window, text, text.Capacity);
        return text.ToString();
    }
    public static string Read(IntPtr window) {
        var lines = new List<string>();
        Callback callback = (child, data) => {
            var text = new StringBuilder(2048);
            GetWindowText(child, text, text.Capacity);
            if (text.Length > 0) lines.Add(text.ToString());
            return true;
        };
        callback(window, IntPtr.Zero);
        EnumChildWindows(window, callback, IntPtr.Zero);
        return string.Join(Environment.NewLine, lines);
    }
    private static bool TryActivate(IntPtr window) {
        IntPtr foreground = GetForegroundWindow();
        if (foreground == window) return true;
        uint owner;
        uint currentThread = GetCurrentThreadId();
        uint windowThread = GetWindowThreadProcessId(window, out owner);
        uint foregroundThread = GetWindowThreadProcessId(foreground, out owner);
        bool attachedCurrent = foregroundThread != 0 && foregroundThread != currentThread &&
            AttachThreadInput(currentThread, foregroundThread, true);
        bool attachedWindow = foregroundThread != 0 && windowThread != 0 &&
            windowThread != currentThread && windowThread != foregroundThread &&
            AttachThreadInput(windowThread, foregroundThread, true);
        try {
            BringWindowToTop(window);
            SetForegroundWindow(window);
            return GetForegroundWindow() == window;
        } finally {
            if (attachedWindow) AttachThreadInput(windowThread, foregroundThread, false);
            if (attachedCurrent) AttachThreadInput(currentThread, foregroundThread, false);
        }
    }
    public static bool ConfirmVerifiedFile(IntPtr window, string fileName) {
        return ConfirmVerifiedFile(window, fileName, TryActivate, IsWindowEnabled,
            button => PostMessage(button, 0x00F5, IntPtr.Zero, IntPtr.Zero));
    }
    public static bool ConfirmVerifiedFile(IntPtr window, string fileName,
        Func<IntPtr, bool> activate, Func<IntPtr, bool> enabled, Func<IntPtr, bool> click) {
        if (Read(window).IndexOf(fileName, StringComparison.OrdinalIgnoreCase) < 0)
            throw new InvalidOperationException("The launch warning does not identify the verified installer.");
        IntPtr runButton = IntPtr.Zero;
        EnumChildWindows(window, (child, data) => {
            var text = new StringBuilder(2048);
            GetWindowText(child, text, text.Capacity);
            if (text.ToString().Replace("&", "").Trim() == "Run") runButton = child;
            return true;
        }, IntPtr.Zero);
        if (runButton == IntPtr.Zero)
            throw new InvalidOperationException("Could not acknowledge the verified installer's Run button.");
        // Match the repository's native UI activation path: SetActiveWindow alone
        // cannot activate a background process. BM_CLICK needs the actual dialog
        // in the foreground before its button can dispatch the Run action.
        if (!activate(window) || !enabled(runButton)) return false;
        if (!click(runButton))
            throw new InvalidOperationException("Could not click the verified installer's Run button.");
        return true;
    }
}
'@
