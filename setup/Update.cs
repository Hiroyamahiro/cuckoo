using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace CuckooSetup {
 public interface IUpdateOptions { bool AppRunning {get;} bool ApproveRunningExit {get;set;} }
 public interface IRunningApplication { bool IsRunning {get;} Task RequestExit(); }

 public static class GracefulUpdate {
  public static async Task EnsureStopped(IRunningApplication app,bool approved,Action<string> progress) {
   if(!app.IsRunning)return;
   if(!approved)throw new SetupError("APP_RUNNING","Cuckooを終了して更新します。「終了して更新」を選択してください。");
   progress("Cuckooの終了を待っています。設定と音源を保存しています。");
   await app.RequestExit();
   if(app.IsRunning)throw new SetupError("APP_RUNNING","Cuckooの終了を確認できないため、更新を保留しました。終了後に再試行してください。");
  }
 }

 public sealed class UpdateGate:IDisposable {
  readonly Semaphore gate;
  // Must match src-tauri/src/updates.rs. ASCII case folding avoids differences
  // between .NET and Rust Unicode case mapping in a user's installation path.
  public static string Name(string root) {
   string p=Path.GetFullPath(root).TrimEnd('\\','/');
   if(p.StartsWith("\\\\?\\",StringComparison.Ordinal))p=p.Substring(4);
   var chars=p.Replace('\\','/').ToCharArray();
   for(int i=0;i<chars.Length;i++)if(chars[i]>='A'&&chars[i]<='Z')chars[i]=(char)(chars[i]+32);
   using(var sha=SHA256.Create())return "Local\\CuckooUpdate-"+BitConverter.ToString(sha.ComputeHash(Encoding.UTF8.GetBytes(new string(chars)))).Replace("-","").ToLowerInvariant();
  }
  public UpdateGate(string root){gate=new Semaphore(1,1,Name(root));if(!gate.WaitOne(0)){gate.Dispose();throw new SetupError("UPDATE_BUSY","別の更新処理が動作しています。終了後に再試行してください。");}}
  public void Dispose(){gate.Release();gate.Dispose();}
 }

 public sealed class RunningApplication:IRunningApplication {
  readonly string exe; readonly TimeSpan timeout;
  public RunningApplication(string exe):this(exe,TimeSpan.FromSeconds(30)){}
  public RunningApplication(string exe,TimeSpan timeout){this.exe=Path.GetFullPath(exe);this.timeout=timeout;}
  List<Process> Capture(){
   var found=new List<Process>();
   try {
    foreach(var p in Process.GetProcessesByName(Path.GetFileNameWithoutExtension(exe))) {
     try {if(p.HasExited){p.Dispose();continue;}if(String.Equals(p.MainModule.FileName,exe,StringComparison.OrdinalIgnoreCase)){var handle=p.Handle;found.Add(p);}else p.Dispose();}
     catch(InvalidOperationException){p.Dispose();}
     catch(System.ComponentModel.Win32Exception){bool exited=false;try{exited=p.HasExited;}catch{}p.Dispose();if(!exited)throw new SetupError("PROCESS_STATE","Cuckooの起動状態を確認できないため、更新を保留しました。");}
    }
    return found;
   }catch{foreach(var p in found)p.Dispose();throw;}
  }
  public bool IsRunning {get{var found=Capture();try{return found.Count!=0;}finally{foreach(var p in found)p.Dispose();}}}
  public async Task RequestExit(){
   var running=Capture();
   try {
    if(running.Count==0)return;
    Safety.RejectReparse(exe);
    var info=FileVersionInfo.GetVersionInfo(exe);
    if(info.FileMajorPart<2||(info.FileMajorPart==2&&info.FileMinorPart==0&&info.FileBuildPart<1))
     throw new SetupError("OLD_EXIT_PROTOCOL","このバージョンは自動終了に対応していません。トレイの「終了」からCuckooを終了して再試行してください。更新は保留しています。");
    using(var request=Process.Start(new ProcessStartInfo(exe,"--request-update-exit"){UseShellExecute=false,CreateNoWindow=true,WorkingDirectory=Path.GetDirectoryName(exe)})) {
     if(request==null)throw new SetupError("EXIT_REQUEST","終了を要求できないため、更新を保留しました。");
     var clock=Stopwatch.StartNew();
     while(clock.Elapsed<timeout) {
      bool exited=request.HasExited;
      foreach(var p in running)exited=exited&&p.HasExited;
      if(exited)return;
      await Task.Delay(100);
     }
    }
    throw new SetupError("EXIT_TIMEOUT","Cuckooの終了を確認できないため、更新を保留しました。強制終了は行いません。トレイの「終了」から終了後、再試行してください。");
   }finally{foreach(var p in running)p.Dispose();}
  }
 }
}
