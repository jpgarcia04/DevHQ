# Dumps the native stack of every thread of a running process, using only Windows' own
# dbghelp.dll and DevHQ's release PDB. Use it on a DevHQ process that does not exit after
# its window closes, BEFORE killing it, so the blocked thread can be identified.
#
#   powershell -NoProfile -File scripts/dump-stacks.ps1 -ProcessId <pid> [-Out stacks.txt]
#
# Rust frames resolve from src-tauri/target/release/devhq.pdb (build with `npm run tauri build`);
# Windows frames resolve to the nearest export.
param([Parameter(Mandatory)] [int]$ProcessId, [string]$Out, [int]$MaxFrames = 40)

$source = @'
// Minimal native stack dumper for a live x64 process, using only Windows' dbghelp.dll.
// C# 5 (Windows PowerShell's Add-Type compiler): no string interpolation.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;

public static class StackDump {
  [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
  [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenThread(uint access, bool inherit, int tid);
  [DllImport("kernel32.dll", SetLastError = true)] static extern uint SuspendThread(IntPtr thread);
  [DllImport("kernel32.dll", SetLastError = true)] static extern uint ResumeThread(IntPtr thread);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetThreadContext(IntPtr thread, IntPtr context);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool CloseHandle(IntPtr handle);
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr LoadLibrary(string name);
  [DllImport("kernel32.dll", CharSet = CharSet.Ansi, SetLastError = true)] static extern IntPtr GetProcAddress(IntPtr module, string name);
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern int GetThreadDescription(IntPtr thread, out IntPtr description);
  [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr memory);
  [DllImport("dbghelp.dll", SetLastError = true)] static extern uint SymSetOptions(uint options);
  [DllImport("dbghelp.dll", CharSet = CharSet.Ansi, SetLastError = true)] static extern bool SymInitialize(IntPtr process, string searchPath, bool invade);
  [DllImport("dbghelp.dll", SetLastError = true)] static extern bool SymCleanup(IntPtr process);
  [DllImport("dbghelp.dll", SetLastError = true)] static extern bool SymFromAddr(IntPtr process, ulong address, out ulong displacement, IntPtr symbol);
  [DllImport("dbghelp.dll", SetLastError = true)] static extern bool StackWalk64(uint machine, IntPtr process, IntPtr thread, IntPtr frame, IntPtr context,
    IntPtr readMemory, IntPtr functionTableAccess, IntPtr getModuleBase, IntPtr translateAddress);

  const uint PROCESS_ALL = 0x001F0FFF;
  const uint THREAD_ACCESS = 0x0002 | 0x0008 | 0x0010 | 0x0040 | 0x0400; // suspend, get/set context, query
  const uint CONTEXT_FULL_AMD64 = 0x10000B;

  class ModuleInfo { public string Name; public long Base; public long Size; }

  public static string Dump(int pid, string symbolPath, int maxFrames) {
    var output = new StringBuilder();
    var process = Process.GetProcessById(pid);
    var modules = new List<ModuleInfo>();
    foreach (ProcessModule module in process.Modules)
      modules.Add(new ModuleInfo { Name = module.ModuleName, Base = module.BaseAddress.ToInt64(), Size = module.ModuleMemorySize });
    IntPtr handle = OpenProcess(PROCESS_ALL, false, pid);
    if (handle == IntPtr.Zero) return "OpenProcess failed: " + Marshal.GetLastWin32Error();
    SymSetOptions(0x00000002 | 0x00000004 | 0x00000010); // undecorate, deferred loads, load lines
    if (!SymInitialize(handle, symbolPath, true)) output.AppendLine("SymInitialize failed: " + Marshal.GetLastWin32Error());
    IntPtr dbghelp = LoadLibrary("dbghelp.dll");
    IntPtr tableAccess = GetProcAddress(dbghelp, "SymFunctionTableAccess64");
    IntPtr moduleBase = GetProcAddress(dbghelp, "SymGetModuleBase64");
    IntPtr raw = Marshal.AllocHGlobal(1232 + 16);
    IntPtr context = new IntPtr((raw.ToInt64() + 15) & ~15L);
    IntPtr frame = Marshal.AllocHGlobal(1024);
    IntPtr symbol = Marshal.AllocHGlobal(88 + 1024);
    try {
      foreach (ProcessThread info in process.Threads) {
        IntPtr thread = OpenThread(THREAD_ACCESS, false, info.Id);
        string description = "";
        IntPtr text;
        if (thread != IntPtr.Zero && GetThreadDescription(thread, out text) >= 0 && text != IntPtr.Zero) {
          description = Marshal.PtrToStringUni(text);
          LocalFree(text);
        }
        string wait = info.ThreadState == System.Diagnostics.ThreadState.Wait ? info.WaitReason.ToString() : "";
        output.AppendLine(string.Format("--- thread {0} [{1}] state={2} {3} cpu={4:F2}s", info.Id, description, info.ThreadState, wait, info.TotalProcessorTime.TotalSeconds));
        if (thread == IntPtr.Zero) { output.AppendLine("    (cannot open thread)"); continue; }
        SuspendThread(thread);
        try {
          for (int i = 0; i < 1232; i += 8) Marshal.WriteInt64(context, i, 0);
          Marshal.WriteInt32(context, 0x30, unchecked((int)CONTEXT_FULL_AMD64));
          if (!GetThreadContext(thread, context)) { output.AppendLine("    GetThreadContext failed: " + Marshal.GetLastWin32Error()); continue; }
          for (int i = 0; i < 1024; i += 8) Marshal.WriteInt64(frame, i, 0);
          Marshal.WriteInt64(frame, 0, Marshal.ReadInt64(context, 0xF8));  // AddrPC = Rip
          Marshal.WriteInt32(frame, 12, 3);
          Marshal.WriteInt64(frame, 32, Marshal.ReadInt64(context, 0xA0)); // AddrFrame = Rbp
          Marshal.WriteInt32(frame, 44, 3);
          Marshal.WriteInt64(frame, 48, Marshal.ReadInt64(context, 0x98)); // AddrStack = Rsp
          Marshal.WriteInt32(frame, 60, 3);
          for (int depth = 0; depth < maxFrames; depth++) {
            if (!StackWalk64(0x8664, handle, thread, frame, context, IntPtr.Zero, tableAccess, moduleBase, IntPtr.Zero)) break;
            ulong pc = (ulong)Marshal.ReadInt64(frame, 0);
            if (pc == 0) break;
            output.AppendLine("    " + Describe(handle, pc, symbol, modules));
          }
        } finally {
          ResumeThread(thread);
          CloseHandle(thread);
        }
      }
    } finally {
      Marshal.FreeHGlobal(raw);
      Marshal.FreeHGlobal(frame);
      Marshal.FreeHGlobal(symbol);
      SymCleanup(handle);
      CloseHandle(handle);
    }
    return output.ToString();
  }

  static string Describe(IntPtr process, ulong pc, IntPtr symbol, List<ModuleInfo> modules) {
    string module = "?";
    foreach (var m in modules)
      if ((long)pc >= m.Base && (long)pc < m.Base + m.Size) { module = m.Name; break; }
    for (int i = 0; i < 88; i += 4) Marshal.WriteInt32(symbol, i, 0);
    Marshal.WriteInt32(symbol, 0, 88);
    Marshal.WriteInt32(symbol, 80, 1000);
    ulong displacement;
    if (SymFromAddr(process, pc, out displacement, symbol)) {
      int length = Marshal.ReadInt32(symbol, 76);
      string name = Marshal.PtrToStringAnsi(new IntPtr(symbol.ToInt64() + 84), Math.Min(length, 1000));
      return string.Format("{0}!{1}+0x{2:x}", module, name, displacement);
    }
    return string.Format("{0}!0x{1:x}", module, pc);
  }
}
'@

if (-not ('StackDump' -as [type])) { Add-Type -TypeDefinition $source }
$release = Join-Path $PSScriptRoot '..\src-tauri\target\release' | Resolve-Path -ErrorAction SilentlyContinue
$symbols = (@($release, "$env:WINDIR\System32") | Where-Object { $_ }) -join ';'
$text = [StackDump]::Dump($ProcessId, $symbols, $MaxFrames)
if ($Out) { Set-Content -Path $Out -Value $text -Encoding utf8; "wrote $Out" } else { $text }
