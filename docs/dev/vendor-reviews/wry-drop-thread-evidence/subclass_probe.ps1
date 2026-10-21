# Tests whether comctl32 subclass helpers work across threads, for issue #83 (wry-drop-thread-review.md).
# Synthetic: a message-only STATIC window on a helper thread; touches no Syrinx profile or window.
$source = @'
using System;
using System.Runtime.InteropServices;
using System.Threading;

public static class SubclassProbe {
    public delegate IntPtr SubProc(IntPtr hwnd, uint msg, IntPtr wp, IntPtr lp, UIntPtr id, UIntPtr data);
    [DllImport("comctl32.dll", EntryPoint="#410")] static extern bool SetWindowSubclass(IntPtr h, SubProc p, UIntPtr id, UIntPtr data);
    [DllImport("comctl32.dll", EntryPoint="#412")] static extern bool RemoveWindowSubclass(IntPtr h, SubProc p, UIntPtr id);
    [DllImport("comctl32.dll", EntryPoint="#413")] static extern IntPtr DefSubclassProc(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr CreateWindowExW(uint ex, string cls, string name, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern int GetMessageW(out MSG m, IntPtr h, uint a, uint b);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessageW(ref MSG m);
    [DllImport("user32.dll")] static extern bool PostThreadMessageW(uint t, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool DestroyWindow(IntPtr h);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [StructLayout(LayoutKind.Sequential)] public struct MSG { public IntPtr h; public uint m; public IntPtr w; public IntPtr l; public uint t; public int x; public int y; }

    const uint PROBE = 0x0400 + 0x65;
    static int hits;
    static SubProc proc = Probe;
    static IntPtr Probe(IntPtr h, uint m, IntPtr w, IntPtr l, UIntPtr id, UIntPtr data) {
        if (m == PROBE) Interlocked.Increment(ref hits);
        return DefSubclassProc(h, m, w, l);
    }

    public static string Run() {
        IntPtr hwnd = IntPtr.Zero; uint owner = 0; bool ownSet = false;
        var ready = new ManualResetEvent(false);
        var thread = new Thread(() => {
            owner = GetCurrentThreadId();
            hwnd = CreateWindowExW(0, "STATIC", "probe", 0, 0, 0, 0, 0, new IntPtr(-3), IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
            ownSet = SetWindowSubclass(hwnd, proc, new UIntPtr(1), UIntPtr.Zero);
            ready.Set();
            MSG msg;
            while (GetMessageW(out msg, IntPtr.Zero, 0, 0) > 0) DispatchMessageW(ref msg);
            DestroyWindow(hwnd);
        });
        thread.Start(); ready.WaitOne();
        SendMessageW(hwnd, PROBE, IntPtr.Zero, IntPtr.Zero);
        int before = hits;
        bool crossSet = SetWindowSubclass(hwnd, proc, new UIntPtr(2), UIntPtr.Zero);
        bool crossRemove = RemoveWindowSubclass(hwnd, proc, new UIntPtr(1));
        SendMessageW(hwnd, PROBE, IntPtr.Zero, IntPtr.Zero);
        int after = hits;
        bool absentRemove = RemoveWindowSubclass(hwnd, proc, new UIntPtr(99));
        PostThreadMessageW(owner, 0x0012, IntPtr.Zero, IntPtr.Zero);
        thread.Join();
        return String.Format("owner-thread set={0}; hits before={1}; cross-thread set={2}; cross-thread remove={3}; hits after={4} (subclass {5}); cross-thread remove of absent id={6}",
            ownSet, before, crossSet, crossRemove, after, after > before ? "still runs" : "gone", absentRemove);
    }
}
'@
Add-Type -TypeDefinition $source
[SubclassProbe]::Run()
(Get-Process -Id $PID).Modules | Where-Object ModuleName -eq "comctl32.dll" | ForEach-Object { "$($_.FileName) $($_.FileVersionInfo.FileVersion)" }
