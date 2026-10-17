using System;
using System.IO;
using System.Diagnostics;
using System.Threading;
using System.Windows.Forms;
using System.Security.Principal;
namespace CuckooSetup {
 static class Program {
  [STAThread] static void Main(string[] args){
   Application.EnableVisualStyles();Application.SetCompatibleTextRenderingDefault(false);
   bool uninstall=Array.IndexOf(args,"/uninstall")>=0||Array.IndexOf(args,"/uninstall-worker")>=0;
#if MAINTENANCE
   uninstall=true;
#endif
   bool quiet=Array.IndexOf(args,"/quiet")>=0,worker=Array.IndexOf(args,"/uninstall-worker")>=0;
   try{
    using(var backend=new Backend()){
     if(uninstall&&!worker){string stage=Safety.Stage();string exe=Path.Combine(stage,"CuckooMaintenance.exe");string own=Application.ExecutablePath;Safety.RejectReparse(own);File.Copy(own,exe,false);if(Safety.HashFile(own)!=Safety.HashFile(exe))throw new SetupError("INTEGRITY","保守画面を準備できませんでした。");Process.Start(new ProcessStartInfo(exe,"/uninstall-worker"+(quiet?" /quiet":"")){UseShellExecute=false,CreateNoWindow=true,WorkingDirectory=stage});return;}
     if(worker){string parent=Path.GetDirectoryName(Application.ExecutablePath),runs=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),"CuckooSetup","Runs");Guid run;if(!String.Equals(Path.GetDirectoryName(parent),runs,StringComparison.OrdinalIgnoreCase)||!Guid.TryParseExact(Path.GetFileName(parent),"N",out run))throw new SetupError("WORKER","保守画面の起動元を確認できません。");Safety.RejectReparse(parent);string installed=Path.Combine(backend.Root,"CuckooMaintenance.exe");if(!File.Exists(installed)||Safety.HashFile(installed)!=Safety.HashFile(Application.ExecutablePath))throw new SetupError("WORKER","保守画面の整合性を確認できません。");}
     string name="Local\\CuckooSetup-"+WindowsIdentity.GetCurrent().User.Value;using(var mutex=new Mutex(false,name)){bool owned=false;try{try{owned=mutex.WaitOne(0);}catch(AbandonedMutexException){owned=true;}if(!owned){if(!quiet)MessageBox.Show("別のセットアップが動作しています。終了後に再試行してください。","Cuckoo",MessageBoxButtons.OK,MessageBoxIcon.Information);Environment.ExitCode=22;return;}
      if(quiet){try{if(uninstall)backend.Uninstall(s=>{}).GetAwaiter().GetResult();else backend.Install(s=>{}).GetAwaiter().GetResult();Environment.ExitCode=0;}catch(Unconfirmed){while(backend.ChildActive)Thread.Sleep(1000);Environment.ExitCode=21;}catch{Environment.ExitCode=20;}}
      else Application.Run(new SetupForm(backend,uninstall,typeof(Program).Assembly.GetName().Version.ToString(3)));
     }finally{if(owned)mutex.ReleaseMutex();}}
    }
   }catch(Exception e){Environment.ExitCode=20;if(!quiet)MessageBox.Show(e.Message,"Cuckoo セットアップ",MessageBoxButtons.OK,MessageBoxIcon.Warning);}
  }
 }
}
