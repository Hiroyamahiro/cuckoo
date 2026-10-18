import volumeSvg from '../icons/tabler/volume.svg?raw';
/** The same dimensions, icon and range track for master and individual gain. */
export class RouteVolume {
  readonly input = document.createElement('input');
  private output = document.createElement('output');
  constructor(host: HTMLElement, commit: (value: number) => void, options: {
    id?: string; outputId?: string; titleId?: string; iconId?: string; label?: string; heading?: boolean;
  } = {}) {
    host.classList.add('route-volume');
    const heading = document.createElement('div');
    heading.className = 'route-volume-heading';
    const title = document.createElement('span');
    if (options.heading) title.classList.add('settings-section-title');
    title.id = options.titleId ?? host.id + '-title';
    title.textContent = options.label ?? '音量';
    this.input.id = options.id ?? host.id + '-input';
    this.output.id = options.outputId ?? host.id + '-value';
    this.input.type = 'range'; this.input.className = 'volume-range';
    this.input.min = '0'; this.input.max = '100'; this.input.step = '1';
    this.input.setAttribute('aria-labelledby', title.id);
    this.output.htmlFor = this.input.id;
    const row = document.createElement('div'); row.className = 'volume-slider';
    const icon = document.createElement('span'); icon.className = 'volume-icon';
    if (options.iconId) icon.id = options.iconId;
    icon.setAttribute('aria-hidden','true'); icon.innerHTML = volumeSvg;
    icon.querySelector('svg')!.setAttribute('focusable','false');
    row.append(icon, this.input); heading.append(title, this.output); host.replaceChildren(heading,row);
    this.input.addEventListener('input', () => this.paint());
    this.input.addEventListener('change', () => commit(Number(this.input.value)));
    this.sync(100, true);
  }
  private paint(): void {
    this.output.value = `${this.input.value}%`;
    this.input.style.setProperty('--volume-fill', `${this.input.value}%`);
  }
  sync(value: number, writable: boolean): void {
    if (document.activeElement !== this.input) this.input.value = String(value);
    this.paint(); this.input.disabled = !writable;
  }
}
