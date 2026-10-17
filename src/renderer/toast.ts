// Transient feedback never participates in document layout. Disk/recovery
// warnings remain in their existing persistent regions.
let timer:ReturnType<typeof setTimeout>|undefined;
export function toast(message:string,tone:'success'|'error'='success'):void{
 let host=document.getElementById('operation-toast');
 if(!host){host=document.createElement('div');host.id='operation-toast';host.setAttribute('popover','manual');document.body.append(host);}
 if(timer)clearTimeout(timer);
 host.dataset.tone=tone;host.setAttribute('role',tone==='error'?'alert':'status');
 host.textContent=message;
 if(!host.matches(':popover-open'))host.showPopover();
 timer=setTimeout(()=>{host.hidePopover();host.textContent='';},tone==='error'?5000:2500);
}
