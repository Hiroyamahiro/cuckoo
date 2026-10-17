using System;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Runtime.Serialization;
using System.Runtime.Serialization.Json;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Win32;
namespace CuckooSetup {
 [DataContract] public sealed class FileRecord { [DataMember] public string Path; [DataMember] public string Sha256; [DataMember] public long Size; }
 [DataContract] public sealed class Payload { [DataMember] public string Version; [DataMember] public FileRecord Engine; [DataMember] public FileRecord WebView; [DataMember] public FileRecord[] Installed; }
 public static class Safety {
  public static string Hash(Stream stream){using(var sha=SHA256.Create())return BitConverter.ToString(sha.ComputeHash(stream)).Replace("-","").ToLowerInvariant();}
  public static string HashFile(string file){RejectReparse(file);using(var s=File.OpenRead(file))return Hash(s);}
  public static void RejectReparse(string path){string p=System.IO.Path.GetFullPath(path);while(!String.IsNullOrEmpty(p)){if((File.Exists(p)||Directory.Exists(p))&&(File.GetAttributes(p)&FileAttributes.ReparsePoint)!=0)throw new SetupError("UNSAFE_PATH","安全でない保存先のため処理を中止しました。");p=System.IO.Path.GetDirectoryName(p);}}
  public static string Child(string root,string relative){if(String.IsNullOrEmpty(relative)||System.IO.Path.IsPathRooted(relative)||relative.Contains(":")||relative.Contains(".."))throw new SetupError("UNSAFE_PATH","安全でないファイル名です。");string p=System.IO.Path.GetFullPath(System.IO.Path.Combine(root,relative));if(!p.StartsWith(System.IO.Path.GetFullPath(root).TrimEnd('\\')+"\\",StringComparison.OrdinalIgnoreCase))throw new SetupError("UNSAFE_PATH","保存領域の外側には書き込みません。");RejectReparse(p);return p;}
  public static string Stage(){string baseDir=System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),"CuckooSetup","Runs");RejectReparse(baseDir);Directory.CreateDirectory(baseDir);string p=System.IO.Path.Combine(baseDir,Guid.NewGuid().ToString("N"));var acl=new DirectorySecurity();var sid=WindowsIdentity.GetCurrent().User;acl.SetOwner(sid);acl.SetAccessRuleProtection(true,false);acl.AddAccessRule(new FileSystemAccessRule(sid,FileSystemRights.FullControl,InheritanceFlags.ContainerInherit|InheritanceFlags.ObjectInherit,PropagationFlags.None,AccessControlType.Allow));Directory.CreateDirectory(p,acl);return p;}
  public static void Cleanup(string directory){if(directory==null)return;string parent=System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),"CuckooSetup","Runs");Guid id;if(!String.Equals(System.IO.Path.GetDirectoryName(directory),parent,StringComparison.OrdinalIgnoreCase)||!Guid.TryParseExact(System.IO.Path.GetFileName(directory),"N",out id))return;try{RejectReparse(directory);foreach(string p in Directory.GetFiles(directory)){RejectReparse(p);File.Delete(p);}Directory.Delete(directory,false);}catch(IOException){}catch(UnauthorizedAccessException){} }
  public static void Verify(string path,FileRecord record){RejectReparse(path);if(!File.Exists(path)||new FileInfo(path).Length!=record.Size||HashFile(path)!=record.Sha256)throw new SetupError("INTEGRITY","ファイルの確認に失敗しました。配布元から取得し直してください。");}
 }
 public static class RuntimePreparation {
  public static async Task Ensure(Func<bool> present,Func<Task<int>> install){if(present())return;int code=await install();if(!present())throw new SetupError("WEBVIEW2","WebView2を導入できませんでした（コード "+code+"）。インターネット接続や管理者の制限を確認してください。");}
 }
 public interface IRemovalOptions {bool RemoveUserData {get;set;}}
 public static class UserDataRemoval {
 public static void Remove(string localData){string root=Safety.Child(localData,Backend.Id);if(!Directory.Exists(root))return;Validate(root);Delete(root);}
 static void Validate(string path){Safety.RejectReparse(path);foreach(var c in Directory.GetFileSystemEntries(path)){Safety.RejectReparse(c);if(Directory.Exists(c))Validate(c);}}
 static void Delete(string path){Safety.RejectReparse(path);foreach(var c in Directory.GetFileSystemEntries(path)){Safety.RejectReparse(c);if(Directory.Exists(c))Delete(c);else File.Delete(c);}Directory.Delete(path,false);}
 }
 public sealed class Backend:IBackend,IDisposable,IRemovalOptions,IUpdateOptions {
  public const string Id="jp.hiroyamahiroshi.cuckoo";
  public const string UninstallKey="Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\"+Id;
  readonly Payload payload; readonly string root; readonly StreamWriter log; Process child; Task outputDrain; string stage; UpdateGate updateGate;
  public bool RemoveUserData {get;set;}
  public bool ApproveRunningExit {get;set;}
  public bool AppRunning {get{try{return new RunningApplication(Path.Combine(root,"Cuckoo.exe")).IsRunning;}catch{return true;}}}
  public string LogPath {get;private set;}
  public string Root {get{return root;}}
  public bool IsUpdate {get{return File.Exists(Path.Combine(root,"Cuckoo.exe"));}}
  public bool RuntimeMissing {get{return !RuntimeInstalled();}}
  public bool ChildActive {get{try{return (child!=null&&!child.HasExited)||(outputDrain!=null&&!outputDrain.IsCompleted);}catch{return child!=null||outputDrain!=null;}}}
  public Backend(){using(var s=Assembly.GetExecutingAssembly().GetManifestResourceStream("Cuckoo.Manifest")){if(s!=null)payload=(Payload)new DataContractJsonSerializer(typeof(Payload)).ReadObject(s);}root=ResolveRoot();Safety.RejectReparse(root);string logs=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),"CuckooSetup","Logs");Safety.RejectReparse(logs);Directory.CreateDirectory(logs);LogPath=Path.Combine(logs,DateTime.UtcNow.ToString("yyyyMMdd-HHmmss")+"-"+Guid.NewGuid().ToString("N")+".log");log=new StreamWriter(new FileStream(LogPath,FileMode.CreateNew,FileAccess.Write,FileShare.Read));log.AutoFlush=true;Write("Cuckoo setup; root="+root);}
  static string ResolveRoot(){string normal=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),"Programs","HiroyamaHiroshi","Cuckoo");using(var k=Registry.CurrentUser.OpenSubKey(UninstallKey)){if(k==null)return normal;string p=(k.GetValue("InstallLocation") as string??normal).Trim('"');p=Path.GetFullPath(p);if(!p.StartsWith(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData)+"\\",StringComparison.OrdinalIgnoreCase))throw new SetupError("LOCATION","既存のインストール先を安全に確認できません。");return p;}}
  void Write(string message){lock(log){log.WriteLine(DateTime.UtcNow.ToString("o")+" "+message);}}
  void RequireOwned(){string marker=Path.Combine(root,".cuckoo-application-id");Safety.RejectReparse(root);if(File.Exists(Path.Combine(root,"Update.exe")))throw new SetupError("FOREIGN_APP","別のアプリがあるため、この場所にはインストールできません。");if(Directory.Exists(root)&&Directory.GetFileSystemEntries(root).Length!=0&&(!File.Exists(marker)||File.ReadAllText(marker)!=Id))throw new SetupError("FOREIGN_APP","既存のアプリを確認できません。別のアプリは上書きしません。");}
  void RequireStopped(){if(new RunningApplication(Path.Combine(root,"Cuckoo.exe")).IsRunning)throw new SetupError("APP_RUNNING","Cuckooが起動中のため処理を保留しました。トレイの「終了」から終了して再試行してください。");}
  public static bool RuntimeInstalled(){string sub="SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";foreach(var hive in new[]{RegistryHive.LocalMachine,RegistryHive.CurrentUser})foreach(var view in new[]{RegistryView.Registry32,RegistryView.Registry64}){try{using(var b=RegistryKey.OpenBaseKey(hive,view))using(var k=b.OpenSubKey(sub)){Version v;string s=k==null?null:k.GetValue("pv") as string;if(Version.TryParse(s,out v)&&v>new Version(0,0,0,0))return true;}}catch(System.Security.SecurityException){}}return false;}
  string Extract(string resource,FileRecord file,string name){string p=Safety.Child(stage,name);using(var s=Assembly.GetExecutingAssembly().GetManifestResourceStream(resource)){if(s==null)throw new SetupError("PAYLOAD","インストール用ファイルがありません。配布元から取得し直してください。");using(var f=new FileStream(p,FileMode.CreateNew,FileAccess.Write,FileShare.None)){s.CopyTo(f);f.Flush(true);}}Safety.Verify(p,file);return p;}
  async Task<int> Run(string file,string args,Action<string> progress){if(ChildActive)throw new Unconfirmed("前の処理が続いています。");var info=new ProcessStartInfo(file,args){UseShellExecute=false,CreateNoWindow=true,WindowStyle=ProcessWindowStyle.Hidden,RedirectStandardOutput=true,RedirectStandardError=true,WorkingDirectory=Path.GetDirectoryName(file)};child=new Process{StartInfo=info};Write("Start "+file+" "+args);try{if(!child.Start())throw new Exception();}catch{child.Dispose();child=null;throw new SetupError("PROCESS_START","処理を開始できませんでした。アクセス権や保護ソフトの設定を確認してください。");}var stdout=child.StandardOutput.ReadToEndAsync();var stderr=child.StandardError.ReadToEndAsync();outputDrain=Task.WhenAll(stdout,stderr);var watch=Stopwatch.StartNew();bool warned=false;while(true){bool exited;try{exited=child.HasExited;}catch{throw new Unconfirmed("処理の終了状態を確認できません。");}if(exited)break;if(watch.Elapsed>TimeSpan.FromMinutes(10)&&!warned){progress("時間がかかっています。処理の終了を確認しています。");warned=true;}await Task.Delay(200);}int code=child.ExitCode;var eof=outputDrain;if(await Task.WhenAny(eof,Task.Delay(5000))!=eof)throw new Unconfirmed("処理は終了しましたが、結果の取得を完了できませんでした。");if(eof.IsFaulted)throw new Unconfirmed("処理結果を読み取れませんでした。");Write("Exit "+code+"\n"+stdout.Result+"\n"+stderr.Result);child.Dispose();child=null;outputDrain=null;return code;}
  public Task Install(Action<string> progress){return Task.Run(async()=>{
   if(payload==null||payload.Engine==null||payload.Installed==null)throw new SetupError("PAYLOAD","インストール用ファイルを確認できません。");
   RequireOwned();
   if(updateGate!=null)throw new Unconfirmed("前の更新処理の終了を確認できません。");
   updateGate=new UpdateGate(root);
   try {
    using(var k=Registry.CurrentUser.OpenSubKey(UninstallKey)){Version oldV,newV;if(k!=null&&Version.TryParse(k.GetValue("DisplayVersion") as string,out oldV)&&Version.TryParse(payload.Version,out newV)&&oldV>newV)throw new SetupError("DOWNGRADE","新しいバージョンが既に入っています。");}
    await GracefulUpdate.EnsureStopped(new RunningApplication(Path.Combine(root,"Cuckoo.exe")),ApproveRunningExit,progress);
    RequireStopped();
    stage=Safety.Stage();
    progress("インストールの準備をしています。");
    string engine=Extract("Cuckoo.Engine",payload.Engine,"engine.exe");
    if(RuntimeMissing){progress("起動に必要なWebView2を導入しています。接続を維持してください。");string wv=Extract("Cuckoo.WebView",payload.WebView,"webview2.exe");await RuntimePreparation.Ensure(RuntimeInstalled,()=>Run(wv,"/silent /install",progress));}
    RequireStopped();progress("Cuckooをインストールしています。");
    int code=await Run(engine,"/S /UPDATE /NS /D="+root,progress);
    if(code!=0)throw new SetupError("INSTALL","インストールを完了できませんでした（コード "+code+"）。空き容量やアクセス権を確認してください。");
    progress("インストール結果を確認しています。");
    try{VerifyInstall();}catch(Exception e){throw new Unconfirmed("配置・登録の結果を確認できませんでした。"+e.Message);}
    Write("Installation verified; settings, presets and custom audio were not opened.");
   }finally{if(!ChildActive){Safety.Cleanup(stage);stage=null;updateGate.Dispose();updateGate=null;}}
  });}
  void VerifyInstall(){foreach(var file in payload.Installed)Safety.Verify(Safety.Child(root,file.Path),file);if(!File.Exists(Path.Combine(root,"uninstall.exe")))throw new SetupError("MISSING_UNINSTALL","削除用ファイルが見つかりません。");using(var k=Registry.CurrentUser.OpenSubKey(UninstallKey)){if(k==null||k.GetValue("DisplayVersion") as string!=payload.Version||k.GetValue("UninstallString") as string!=MaintenanceCommand(root,false))throw new SetupError("REGISTRATION","Windowsへの登録を確認できません。");}VerifyShortcut();}
  public static string MaintenanceCommand(string root,bool quiet){return "\""+Path.Combine(root,"CuckooMaintenance.exe")+"\" /uninstall"+(quiet?" /quiet":"");}
  void VerifyShortcut(){Exception error=null;var t=new Thread(()=>{object shell=null,link=null;try{string file=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs),"Cuckoo.lnk");Safety.RejectReparse(file);if(!File.Exists(file))throw new Exception("スタートメニューのCuckooがありません。");var type=Type.GetTypeFromProgID("WScript.Shell");shell=Activator.CreateInstance(type);link=type.InvokeMember("CreateShortcut",BindingFlags.InvokeMethod,null,shell,new object[]{file});string target=link.GetType().InvokeMember("TargetPath",BindingFlags.GetProperty,null,link,null) as string;if(!String.Equals(target,Path.Combine(root,"Cuckoo.exe"),StringComparison.OrdinalIgnoreCase))throw new Exception("ショートカットの参照先が一致しません。");}catch(Exception e){error=e;}finally{if(link!=null)Marshal.FinalReleaseComObject(link);if(shell!=null)Marshal.FinalReleaseComObject(shell);}});t.IsBackground=true;t.SetApartmentState(ApartmentState.STA);t.Start();if(!t.Join(30000))throw new Unconfirmed("ショートカットの確認に時間がかかっています。");if(error!=null)throw error;}
  public Task Uninstall(Action<string> progress){return Task.Run(async()=>{RequireOwned();RequireStopped();string engine=Path.Combine(root,"uninstall.exe");if(!File.Exists(engine))throw new SetupError("UNINSTALL","削除用ファイルがありません。インストーラーで更新してから再試行してください。");Safety.RejectReparse(engine);progress("Cuckooを削除しています。設定と追加した音源は残します。");int code=await Run(engine,"/S",progress);if(code!=0)throw new SetupError("UNINSTALL","削除を完了できませんでした（コード "+code+"）。Cuckooを終了して再試行してください。");for(int i=0;i<300;i++){bool registered;using(var check=Registry.CurrentUser.OpenSubKey(UninstallKey)){registered=check!=null;}if(!registered&&!File.Exists(Path.Combine(root,"Cuckoo.exe"))&&!File.Exists(Path.Combine(root,"CuckooMaintenance.exe")))break;await Task.Delay(100);}using(var key=Registry.CurrentUser.OpenSubKey(UninstallKey)){if(key!=null||File.Exists(Path.Combine(root,"Cuckoo.exe")))throw new Unconfirmed("削除結果を確認できませんでした。");}string shortcut=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs),"Cuckoo.lnk");if(File.Exists(shortcut)){// Foreign shortcuts must survive; ownership is checked by the internal engine.
 Write("Start menu contains a shortcut; may belong to another application.");}if(RemoveUserData){progress("設定と追加した音源を削除しています。");try{UserDataRemoval.Remove(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData));Write("Explicit user-data removal verified.");}catch(Exception e){throw new Unconfirmed("アプリは削除しましたが、設定と追加音源の削除を完了できませんでした。"+e.Message);}}else Write("Uninstall verified; settings were never opened.");});}
  public Task Launch(){return Task.Run(async()=>{RequireOwned();string exe=Path.Combine(root,"Cuckoo.exe");Safety.RejectReparse(exe);if(payload!=null){foreach(var f in payload.Installed)if(f.Path=="Cuckoo.exe")Safety.Verify(exe,f);}Process.Start(new ProcessStartInfo(exe){UseShellExecute=false,CreateNoWindow=true,WorkingDirectory=root});for(int i=0;i<75;i++){foreach(var p in Process.GetProcessesByName("Cuckoo")){using(p){try{if(String.Equals(p.MainModule.FileName,exe,StringComparison.OrdinalIgnoreCase)&&p.MainWindowHandle!=IntPtr.Zero)return;}catch{}}}await Task.Delay(200);}throw new SetupError("LAUNCH","起動を確認できませんでした。スタートメニューから起動するか、再試行してください。");});}
  public void Dispose(){if(!ChildActive){Safety.Cleanup(stage);if(updateGate!=null){updateGate.Dispose();updateGate=null;}log.Dispose();}}
 }
}

