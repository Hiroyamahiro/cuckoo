import { chimeTime, parseChimeTime } from '../custom-chimes';
import { TimeWheel } from './time-wheel';
export class TimeInput {
  private values=[0,0];
  private buttons: HTMLButtonElement[]=[];
  private wheels: TimeWheel[]=[];
  private panel=document.createElement('div');
  constructor(private host: HTMLElement) {
    host.classList.add('time-input');host.setAttribute('role','group');host.setAttribute('aria-label','時刻');
    const panel=this.panel;
    panel.id=host.id+'-picker';panel.className='time-picker';panel.setAttribute('popover','manual');
    panel.setAttribute('role','group');panel.setAttribute('aria-label','時刻を選択');
    const band=document.createElement('div');band.className='time-picker-band';band.setAttribute('aria-hidden','true');panel.append(band);
    (host.closest('dialog')??document.body).append(panel);
    for(const [index,limit]of [24,60].entries()) {
      const button=document.createElement('button');button.type='button';button.id=`${host.id}-${index?'minute':'hour'}`;
      button.setAttribute('aria-haspopup','listbox');button.setAttribute('aria-expanded','false');
      button.setAttribute('aria-controls',`${host.id}-wheel-${index}`);
      button.addEventListener('click',()=>this.open(index));
      button.addEventListener('keydown',event=>{
        if(event.key==='ArrowUp'||event.key==='ArrowDown'){event.preventDefault();this.open(index);}
      });
      if(index){const separator=document.createElement('span');separator.textContent=':';separator.setAttribute('aria-hidden','true');host.append(separator);}
      host.append(button);this.buttons.push(button);
      const wheel=new TimeWheel(`${host.id}-wheel-${index}`,limit,index?'分':'時',value=>{this.values[index]=value;this.paint();});
      wheel.element.addEventListener('keydown',event=>{
        if(event.key==='ArrowLeft'||event.key==='ArrowRight'){event.preventDefault();this.wheels[1-index].element.focus();}
        if(event.key==='Escape'||event.key==='Enter'){event.preventDefault();event.stopPropagation();this.close(true,index);}
      });
      if(index){const separator=document.createElement('span');separator.className='time-picker-separator';separator.textContent=':';separator.setAttribute('aria-hidden','true');panel.append(separator);}
      panel.append(wheel.element);this.wheels.push(wheel);
    }
    this.paint();
    document.addEventListener('pointerdown',event=>{if(!host.contains(event.target as Node)&&!panel.contains(event.target as Node))this.close();});
    panel.addEventListener('focusout',event=>{
      if(event.relatedTarget&&!panel.contains(event.relatedTarget as Node)&&!host.contains(event.relatedTarget as Node))this.close();
    });
    host.closest('dialog')?.addEventListener('close',()=>this.close());
    window.addEventListener('resize',()=>this.close());
    document.addEventListener('scroll',event=>{if(!panel.contains(event.target as Node))this.close();},true);
  }
  private paint(): void {
    this.buttons.forEach((button,index)=>{button.value=String(this.values[index]).padStart(2,'0');button.textContent=button.value;button.setAttribute('aria-label',`${button.value}${index?'分':'時'}`);});
  }
  private open(index: number): void {
    if(this.buttons[index].disabled)return;
    if(!this.panel.matches(':popover-open')){
      this.panel.showPopover();this.wheels.forEach((wheel,i)=>wheel.setValue(this.values[i]));
    }
    const anchor=this.host.getBoundingClientRect(),box=this.panel.getBoundingClientRect();
    const left=Math.max(12,Math.min(anchor.x,innerWidth-box.width-12));
    const top=anchor.bottom+8+box.height<=innerHeight-12?anchor.bottom+8:Math.max(12,anchor.top-box.height-8);
    this.panel.style.left=`${left}px`;this.panel.style.top=`${top}px`;
    this.buttons.forEach(button=>button.setAttribute('aria-expanded','true'));this.wheels[index].element.focus({preventScroll:true});
  }
  private close(focus=false,index=0): void {
    if(!this.panel.matches(':popover-open'))return;
    this.wheels.forEach(wheel=>wheel.finish());
    this.panel.hidePopover();this.buttons.forEach(button=>button.setAttribute('aria-expanded','false'));
    if(focus)this.buttons[index].focus({preventScroll:true});
  }
  set value(time: string) {this.close();const minute=parseChimeTime(time);this.values=[Math.floor(minute/60),minute%60];this.paint();}
  get value(): string {return chimeTime(this.values[0]*60+this.values[1]);}
  focus(): void {this.buttons[0].focus();}
}
