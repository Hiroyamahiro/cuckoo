using System;
using System.Threading;
using System.Threading.Tasks;
namespace CuckooSetup {
 public enum Phase { Ready, Working, Complete, Failed, Unconfirmed, Launching }
 public class SetupError : Exception { public readonly string Code; public SetupError(string code,string message):base(message){Code=code;} }
 public sealed class Unconfirmed : SetupError { public Unconfirmed(string message):base("RESULT_UNCONFIRMED",message){} }
 public interface IBackend { bool IsUpdate {get;} bool RuntimeMissing {get;} bool ChildActive {get;} string LogPath {get;} Task Install(Action<string> progress); Task Uninstall(Action<string> progress); Task Launch(); }
 public sealed class Flow {
  readonly IBackend backend; readonly bool uninstall; int busy;
  public Phase State {get;private set;} public string Detail {get;private set;} public string Error {get;private set;} public bool Restarted {get;private set;} public event Action Changed;
  public bool CanClose {get{return State!=Phase.Working && State!=Phase.Launching && !backend.ChildActive;}}
  public Flow(IBackend backend,bool uninstall){this.backend=backend;this.uninstall=uninstall;State=Phase.Ready;}
  void Notify(){var h=Changed;if(h!=null)h();}
  public async Task Run(bool approveRunningExit=false){if(Interlocked.Exchange(ref busy,1)!=0)return; bool restart=!uninstall&&backend.IsUpdate;var update=backend as IUpdateOptions;if(update!=null)update.ApproveRunningExit=!uninstall&&approveRunningExit;State=Phase.Working;Error=null;Restarted=false;Notify();
   try{Action<string> p=s=>{Detail=s;Notify();};if(uninstall)await backend.Uninstall(p);else await backend.Install(p);State=Phase.Complete;
    if(restart){State=Phase.Launching;Notify();try{await backend.Launch();Restarted=true;}catch(Exception e){Error=e.Message;}State=Phase.Complete;}
   }
   catch(Unconfirmed e){Error=e.Message;State=Phase.Unconfirmed;}
   catch(Exception e){Error=e.Message;State=backend.ChildActive?Phase.Unconfirmed:Phase.Failed;}
   finally{Notify();if(State!=Phase.Unconfirmed)Interlocked.Exchange(ref busy,0);}
  }
  public async Task Launch(){if(State!=Phase.Complete||uninstall||Interlocked.Exchange(ref busy,1)!=0)return;State=Phase.Launching;Error=null;Notify();try{await backend.Launch();Restarted=true;State=Phase.Complete;}catch(Exception e){State=Phase.Complete;Error=e.Message;}finally{Interlocked.Exchange(ref busy,0);Notify();}}
 }
}
