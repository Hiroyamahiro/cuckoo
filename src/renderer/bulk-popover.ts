import adjustmentsHorizontal from '../icons/tabler/adjustments-horizontal.svg?raw';

export class BulkPopover {
  private readonly button = document.querySelector<HTMLButtonElement>('#bulk-toggle')!;
  private readonly popup = document.querySelector<HTMLElement>('#bulk-popover')!;

  constructor() {
    this.button.insertAdjacentHTML('afterbegin', adjustmentsHorizontal);
    const icon = this.button.querySelector('svg')!;
    icon.setAttribute('aria-hidden', 'true');
    icon.setAttribute('focusable', 'false');
    this.popup.addEventListener('beforetoggle', event => {
      if ((event as ToggleEvent).newState !== 'open') return;
      const r = this.button.getBoundingClientRect();
      this.popup.style.setProperty('--bulk-center', `${r.left + r.width / 2}px`);
      this.popup.style.setProperty('--bulk-top', `${r.bottom + 6}px`);
    });
    this.popup.addEventListener('toggle', () => {
      const open = this.popup.matches(':popover-open');
      this.button.setAttribute('aria-expanded', String(open));
      if (!open) return;
      const r = this.button.getBoundingClientRect();
      const height = this.popup.getBoundingClientRect().height;
      const top = r.bottom + 6 + height <= innerHeight - 8
        ? r.bottom + 6 : Math.max(8, r.top - height - 6);
      this.popup.style.setProperty('--bulk-top', `${top}px`);
      this.popup.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus({preventScroll:true});
    });
    const close = () => this.close();
    window.addEventListener('resize', close);
    const scroll = (event: Event) => {
      if (!(event.target instanceof Node) || !this.popup.contains(event.target)) this.close();
    };
    document.addEventListener('scroll', scroll, true);
    const mode = new MutationObserver(() => {
      if (document.documentElement.dataset.minimum === 'true') this.close();
    });
    mode.observe(document.documentElement, {attributes:true, attributeFilter:['data-minimum']});
    window.addEventListener('unload', () => {
      mode.disconnect();
      window.removeEventListener('resize', close);
      document.removeEventListener('scroll', scroll, true);
    }, {once:true});
  }

  setWritable(writable: boolean): void {
    this.button.disabled = !writable;
    if (!writable) this.close();
  }

  close(): void {
    if (!this.popup.matches(':popover-open')) return;
    this.popup.hidePopover();
  }
}
