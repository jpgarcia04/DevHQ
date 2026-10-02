# Uninstalls DevHQ as a person does, with "Delete the application data" ticked on the
# uninstaller's confirmation page, which a silent uninstall never shows.
#
# The page belongs to the NSIS template of tauri-bundler: when it appears, the uninstaller adds a
# plain Win32 checkbox to it, unticked, and when the page is left it reads the box with
# BM_GETCHECK. So BM_SETCHECK sets exactly what a click would, and WM_COMMAND with IDOK is the
# Uninstall button. Text is read with WM_GETTEXT, because GetWindowText cannot read the controls
# of another process, and with a timeout, so a window that does not answer cannot hang the step.
# Returns once the uninstaller has finished and closed; what it deleted is for the caller to check.
param([Parameter(Mandatory)] [string] $Uninstaller)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class NsisWindow {
    delegate bool EnumProc(IntPtr window, IntPtr data);

    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EnumProc callback, IntPtr data);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetClassNameW(IntPtr window, StringBuilder name, int capacity);
    [DllImport("user32.dll", EntryPoint = "SendMessageTimeoutW")]
    static extern IntPtr SendTimeout(IntPtr window, uint message, IntPtr w, IntPtr l, uint flags, uint milliseconds, out IntPtr result);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "SendMessageTimeoutW")]
    static extern IntPtr SendTextTimeout(IntPtr window, uint message, IntPtr capacity, StringBuilder text, uint flags, uint milliseconds, out IntPtr result);
    [DllImport("user32.dll", EntryPoint = "PostMessageW")]
    static extern bool PostMessage(IntPtr window, uint message, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr dialog, int id);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);

    const uint WM_GETTEXT = 0x000D, WM_GETTEXTLENGTH = 0x000E, WM_COMMAND = 0x0111;
    const uint BM_GETCHECK = 0x00F0, BM_SETCHECK = 0x00F1;
    const uint SMTO_ABORTIFHUNG = 0x0002, TIMEOUT_MS = 2000;
    // The dialog's Next button, which NSIS labels Uninstall on this page and Close at the end.
    public const int IDOK = 1;

    static string ClassOf(IntPtr window) {
        var name = new StringBuilder(256);
        GetClassNameW(window, name, name.Capacity);
        return name.ToString();
    }

    static IntPtr Send(IntPtr window, uint message, IntPtr w, IntPtr l) {
        IntPtr result;
        if (SendTimeout(window, message, w, l, SMTO_ABORTIFHUNG, TIMEOUT_MS, out result) == IntPtr.Zero)
            throw new TimeoutException("The window did not answer message 0x" + message.ToString("X4"));
        return result;
    }

    public static string TextOf(IntPtr window) {
        int length = (int)Send(window, WM_GETTEXTLENGTH, IntPtr.Zero, IntPtr.Zero);
        var text = new StringBuilder(length + 1);
        IntPtr copied;
        if (SendTextTimeout(window, WM_GETTEXT, (IntPtr)text.Capacity, text, SMTO_ABORTIFHUNG, TIMEOUT_MS, out copied) == IntPtr.Zero)
            throw new TimeoutException("The window did not answer WM_GETTEXT");
        return text.ToString();
    }

    /// The checkbox labelled `label` in a dialog, or IntPtr.Zero. Only dialogs are searched, and
    /// their class name is read without sending them anything.
    public static IntPtr FindCheckbox(string label, out IntPtr dialog) {
        var dialogs = new List<IntPtr>();
        EnumWindows((window, _) => {
            if (ClassOf(window) == "#32770") dialogs.Add(window);
            return true;
        }, IntPtr.Zero);
        foreach (var candidate in dialogs) {
            var buttons = new List<IntPtr>();
            // Every descendant: the box sits on the page, a dialog inside the uninstaller's.
            EnumChildWindows(candidate, (window, _) => {
                if (string.Equals(ClassOf(window), "Button", StringComparison.OrdinalIgnoreCase)) buttons.Add(window);
                return true;
            }, IntPtr.Zero);
            foreach (var button in buttons) {
                try {
                    if (TextOf(button) == label) { dialog = candidate; return button; }
                } catch (TimeoutException) { }
            }
        }
        dialog = IntPtr.Zero;
        return IntPtr.Zero;
    }

    public static bool IsTicked(IntPtr checkbox) { return Send(checkbox, BM_GETCHECK, IntPtr.Zero, IntPtr.Zero) == (IntPtr)1; }

    public static void Tick(IntPtr checkbox) { Send(checkbox, BM_SETCHECK, (IntPtr)1, IntPtr.Zero); }

    /// Clicks the dialog's Next button as its own message loop sees a click: posted, so that a
    /// message box it might open cannot block this process.
    public static void PressNext(IntPtr dialog) {
        if (!PostMessage(dialog, WM_COMMAND, (IntPtr)IDOK, GetDlgItem(dialog, IDOK)))
            throw new InvalidOperationException("The click could not be posted");
    }

    public static int ProcessOf(IntPtr window) {
        uint process;
        GetWindowThreadProcessId(window, out process);
        return (int)process;
    }

    /// What the other visible dialogs of `process` say, such as a message box: the text of
    /// their static controls.
    public static List<string> OtherDialogs(int process, IntPtr except) {
        var found = new List<IntPtr>();
        EnumWindows((window, _) => {
            if (window != except && IsWindowVisible(window) && ClassOf(window) == "#32770" && ProcessOf(window) == process)
                found.Add(window);
            return true;
        }, IntPtr.Zero);
        var texts = new List<string>();
        foreach (var dialog in found) {
            EnumChildWindows(dialog, (window, _) => {
                if (string.Equals(ClassOf(window), "Static", StringComparison.OrdinalIgnoreCase)) {
                    try { texts.Add(TextOf(window)); } catch (TimeoutException) { }
                }
                return true;
            }, IntPtr.Zero);
        }
        return texts;
    }
}
'@

$label = 'Delete the application data'
if (Get-Process devhq -ErrorAction SilentlyContinue) {
  throw 'DevHQ is running, so the uninstaller would first ask to close it.'
}
# Without /S, as a person starts it. It copies itself to TMP, starts the copy and exits.
Start-Process -FilePath $Uninstaller

$dialog = [IntPtr]::Zero
$checkbox = [IntPtr]::Zero
$deadline = (Get-Date).AddMinutes(1)
while ($checkbox -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline) {
  Start-Sleep -Milliseconds 500
  $checkbox = [NsisWindow]::FindCheckbox($label, [ref]$dialog)
}
if ($checkbox -eq [IntPtr]::Zero) { throw "No window with a '$label' checkbox appeared." }
$process = Get-Process -Id ([NsisWindow]::ProcessOf($dialog))
"The uninstaller ($($process.ProcessName), process $($process.Id)) shows '$([NsisWindow]::TextOf($dialog))'."
if ([NsisWindow]::IsTicked($checkbox)) { throw "'$label' was already ticked." }
"'$label' appears unticked."
[NsisWindow]::Tick($checkbox)
if (-not [NsisWindow]::IsTicked($checkbox)) { throw "'$label' did not take the tick." }
$next = [NsisWindow]::GetDlgItem($dialog, [NsisWindow]::IDOK)
"Ticked it; pressing '$([NsisWindow]::TextOf($next))'."
[NsisWindow]::PressNext($dialog)

# The confirmation page, checkbox included, goes away when the uninstall starts. The Close
# button that replaces Uninstall stays disabled until the uninstall has finished, and then waits
# for a click. A click that arrives while it is disabled is ignored, so keep clicking.
$deadline = (Get-Date).AddMinutes(2)
$pressed = ''
while (-not $process.HasExited -and (Get-Date) -lt $deadline) {
  Start-Sleep -Milliseconds 500
  $other = [NsisWindow]::OtherDialogs($process.Id, $dialog)
  if ($other.Count) {
    Stop-Process -Id $process.Id -Force
    throw "The uninstaller showed a message: $($other -join ' ')"
  }
  if ([NsisWindow]::IsWindow($checkbox) -or -not [NsisWindow]::IsWindow($dialog)) { continue }
  $next = [NsisWindow]::GetDlgItem($dialog, [NsisWindow]::IDOK)
  if ([NsisWindow]::IsWindowEnabled($next)) {
    $text = [NsisWindow]::TextOf($next)
    if ($text -ne $pressed) { "The uninstaller finished; pressing '$text'." }
    $pressed = $text
    [NsisWindow]::PressNext($dialog)
  }
}
if (-not $process.HasExited) {
  Stop-Process -Id $process.Id -Force
  throw 'The uninstaller did not finish and close within two minutes.'
}
'The uninstaller closed.'
