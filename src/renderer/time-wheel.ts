import { stepTimePart } from '../custom-chimes';
/** Independent cyclic column, with native touch scrolling and a centered selection. */
export class TimeWheel {
  readonly element = document.createElement('div');
  private options: HTMLElement[] = [];
  private selected = -1;
  private target = 0;
  private moving = false;
  private settle?: ReturnType<typeof setTimeout>;
  constructor(id: string, private limit: number, label: string, private commit: (value: number) => void) {
    const column=this.element;
    column.id=id;column.className='time-wheel';column.tabIndex=0;
    column.setAttribute('role','listbox');column.setAttribute('aria-label',label);
    for(let index=0;index<limit*5;index++) {
      const option=document.createElement('div');option.className='time-wheel-option';
      option.id=`${id}-${index}`;option.dataset.index=String(index);
      option.setAttribute('role','option');option.setAttribute('aria-selected','false');
      option.textContent=String(index%limit).padStart(2,'0');column.append(option);this.options.push(option);
    }
    column.addEventListener('scroll',()=>{
      if(!column.closest('.time-picker')?.matches(':popover-open'))return;
      const index=Math.max(0,Math.min(this.options.length-1,Math.round(column.scrollTop/this.rowHeight)));
      this.paint(index);if(!this.moving)this.target=index;
      clearTimeout(this.settle);
      this.settle=setTimeout(()=>{
        if(!column.closest('.time-picker')?.matches(':popover-open'))return;
        this.moving=false;this.target=index;if(index<limit||index>=limit*4)this.setValue(index%limit);
      },160);
    });
    let wheel=0;
    column.addEventListener('wheel',event=>{
      if(!event.deltaY)return;event.preventDefault();column.focus({preventScroll:true});
      const delta=event.deltaY*(event.deltaMode===1?16:event.deltaMode===2?100:1);
      if(Math.sign(delta)!==Math.sign(wheel))wheel=0;wheel+=delta;
      if(Math.abs(wheel)>=40){this.move(wheel>0?1:-1);wheel=0;}
    },{passive:false});
    column.addEventListener('keydown',event=>{
      if(event.key==='ArrowUp'||event.key==='ArrowDown'){event.preventDefault();this.move(event.key==='ArrowDown'?1:-1);}
      if(event.key==='Home'||event.key==='End'){event.preventDefault();this.setValue(event.key==='Home'?0:limit-1);}
    });
    let drag:{id:number;y:number;top:number;moved:boolean}|undefined;
    let suppressClick=false;
    column.addEventListener('pointerdown',event=>{
      if(event.button!==0||event.pointerType==='touch')return;
      column.focus({preventScroll:true});this.moving=false;
      column.scrollTo({top:column.scrollTop,behavior:'instant'});
      drag={id:event.pointerId,y:event.clientY,top:column.scrollTop,moved:false};
    });
    column.addEventListener('pointermove',event=>{
      if(!drag||drag.id!==event.pointerId)return;
      if(Math.abs(event.clientY-drag.y)>4){drag.moved=true;column.setPointerCapture(event.pointerId);}
      if(drag.moved){event.preventDefault();column.scrollTop=drag.top+drag.y-event.clientY;}
    });
    const end=(event:PointerEvent)=>{
      if(!drag||event.pointerId!==drag.id)return;
      suppressClick=drag.moved;drag=undefined;
      if(column.hasPointerCapture(event.pointerId))column.releasePointerCapture(event.pointerId);
      this.moveTo(Math.round(column.scrollTop/this.rowHeight));
    };
    column.addEventListener('pointerup',end);column.addEventListener('pointercancel',end);
    column.addEventListener('click',event=>{
      if(suppressClick){suppressClick=false;return;}
      const option=(event.target as HTMLElement).closest<HTMLElement>('[data-index]');
      if(option)this.moveTo(Number(option.dataset.index));
    });
  }
  private get rowHeight(): number {return parseFloat(getComputedStyle(this.element).getPropertyValue('--time-wheel-row-height'));}
  private paint(index: number): void {
    if(this.selected>=0)this.options[this.selected].setAttribute('aria-selected','false');
    this.selected=index;this.options[index].setAttribute('aria-selected','true');
    this.element.setAttribute('aria-activedescendant',this.options[index].id);
    this.options.forEach((option,i)=>option.dataset.distance=String(Math.min(Math.abs(i-index),3)));
    this.commit(index%this.limit);
  }
  private moveTo(index: number): void {
    this.target=Math.max(0,Math.min(this.options.length-1,index));this.moving=true;
    this.element.scrollTo({top:this.target*this.rowHeight,behavior:matchMedia('(prefers-reduced-motion: reduce)').matches?'instant':'smooth'});
  }
  private move(step: number): void {
    if(this.target<=0||this.target>=this.options.length-1)this.setValue(stepTimePart(this.target,0,this.limit));
    this.moveTo(this.target+step);
  }
  setValue(value: number): void {
    clearTimeout(this.settle);this.moving=false;this.target=this.limit*2+value;
    this.element.scrollTo({top:this.target*this.rowHeight,behavior:'instant'});this.paint(this.target);
  }
  finish(): void {
    if(this.selected>=0)this.setValue((this.moving?this.target:this.selected)%this.limit);
  }
}
