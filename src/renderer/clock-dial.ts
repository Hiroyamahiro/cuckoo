import {
  dialSize,
  dialCenter,
  customRingDots,
  customRingRadius,
  hitHour,
  paintHours,
  point,
  ringRadius,
  slotTooltip,
  type Ring,
} from './dial-model';
interface Gesture {
  id: number;
  ring: Ring;
  previous: number | null;
  enabled: boolean;
  original: boolean[];
}
import type { CustomChime } from '../shared/settings';
import { customMinutes, customMinuteTooltip, customMinutesOnArc, setCustomMinutes } from '../custom-chimes';
export class ClockDial {
  private readonly customRing = document.createElementNS('http://www.w3.org/2000/svg', 'g');
  private readonly customButtons = new Map<number, HTMLButtonElement>();
  private readonly customMarkers = new Map<number, SVGCircleElement>();
  private disabled = true;
  private slots = Array<boolean>(48).fill(false);
  private readonly buttons: HTMLButtonElement[] = [];
  private gesture: Gesture | null = null;
  private customChimes: readonly CustomChime[] = [];
  private customGesture: {id: number; previous: number | null; enabled: boolean; selected: Set<number>; original: readonly CustomChime[]} | null = null;
  private readonly hand: SVGLineElement;
  constructor(
    private readonly host: HTMLElement,
    private readonly commit: (slots: boolean[]) => void,
    private readonly preview: (slots: readonly boolean[]) => void = () => {},
    private readonly setCustom: (minutes: readonly number[], enabled: boolean) => Promise<void> = async () => {},
  ) {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('viewBox', `0 0 ${dialSize} ${dialSize}`);
    svg.setAttribute('aria-hidden', 'true');
    this.customRing.classList.add('custom-chime-ring');
    this.customRing.style.display = 'none';
    for (let index = 0; index < customRingDots; index++) {
      const dot = document.createElementNS(svg.namespaceURI, 'circle');
      const location = point(index / customRingDots * 24, customRingRadius);
      dot.setAttribute('cx', String(location.x)); dot.setAttribute('cy', String(location.y));
      dot.setAttribute('r', '1'); dot.classList.add('custom-ring-dot'); this.customRing.append(dot);
    }
    svg.append(this.customRing);
    for (const radius of [ringRadius(0), ringRadius(1)]) {
      const circle = document.createElementNS(svg.namespaceURI, 'circle');
      circle.setAttribute('cx', String(dialCenter));
      circle.setAttribute('cy', String(dialCenter));
      circle.setAttribute('r', String(radius));
      svg.append(circle);
    }
    this.hand = document.createElementNS('http://www.w3.org/2000/svg', 'line');
    this.hand.classList.add('current-hand');
    for (const [name, value] of Object.entries({
      x1: String(dialCenter),
      // 中央の情報表示を避け、半径80pxから針を描く。
      y1: String(dialCenter - 80),
      x2: String(dialCenter),
      y2: String(dialCenter - 132),
    }))
      this.hand.setAttribute(name, value);
    svg.append(this.hand);
    host.prepend(svg);
    // Keep hover feedback below every dial marker and label.
    const hover = document.createElement('span');
    hover.className = 'dial-hover-background';
    hover.setAttribute('aria-hidden', 'true');
    hover.hidden = true;
    host.prepend(hover);
    const updateHover = () => {
      const button = host.querySelector<HTMLButtonElement>('.slot:hover:not(:disabled),.custom-slot:hover:not(:disabled)')
        ?? host.querySelector<HTMLButtonElement>('.slot:focus-visible:not(:disabled),.custom-slot:focus-visible:not(:disabled)');
      hover.hidden = !button;
      if (!button) return;
      hover.style.left = button.style.left;
      hover.style.top = button.style.top;
      hover.style.width = button.offsetWidth + 'px';
      hover.style.height = button.offsetHeight + 'px';
    };
    for (const event of ['pointerover', 'pointerout', 'focusin', 'focusout']) host.addEventListener(event, updateHover);
    for (let hour = 0; hour < 24; hour++) {
      const label = document.createElement('span');
      label.className = 'hour-label';
      label.textContent = String(hour).padStart(2, '0');
      this.position(label, hour, 149);
      host.append(label);
      for (const ring of [0, 1] as const) {
        const slot = hour * 2 + ring;
        const button = document.createElement('button');
        button.type = 'button';
        button.className = `slot ${ring === 0 ? 'hourly' : 'half-hourly'}`;
        button.dataset.slot = String(slot);
        button.dataset.ring = String(ring);
        button.setAttribute(
          'aria-label',
          `${String(hour).padStart(2, '0')}:${ring === 0 ? '00' : '30'}の時報`,
        );
        this.position(button, hour + ring * 0.5, ringRadius(ring));
        button.addEventListener('click', (event) => {
          if (event.detail !== 0 || this.gesture) return;
          this.slots[slot] = !this.slots[slot];
          this.paint();
          this.commit([...this.slots]);
        });
        this.buttons[slot] = button;
        host.append(button);
      }
    }
    host.addEventListener('pointerdown', (event) => {
      const button = (event.target as HTMLElement).closest<HTMLButtonElement>(
        '.slot,.custom-slot',
      );
      if (!button || button.disabled || this.gesture || this.customGesture || event.button !== 0)
        return;
      event.preventDefault();
      button.focus({ preventScroll: true });
      if (button.classList.contains('custom-slot')) {
        const minute = Number(button.dataset.minute);
        this.customGesture = {id: event.pointerId, previous: minute, enabled: button.dataset.active !== 'true', selected: new Set([minute]), original: this.customChimes};
        host.setPointerCapture(event.pointerId);
        this.paintCustomGesture();
        return;
      }
      const slot = Number(button.dataset.slot);
      this.gesture = {
        id: event.pointerId,
        ring: (slot % 2) as Ring,
        previous: Math.floor(slot / 2),
        enabled: !this.slots[slot],
        original: [...this.slots],
      };
      host.setPointerCapture(event.pointerId);
      this.slots[slot] = this.gesture.enabled;
      this.paint();
    });
    host.addEventListener('pointermove', (event) => {
      const custom = this.customGesture;
      if (custom && custom.id === event.pointerId) {
        const bounds = host.getBoundingClientRect();
        const x = (event.clientX - bounds.left) * dialSize / bounds.width - dialCenter;
        const y = (event.clientY - bounds.top) * dialSize / bounds.height - dialCenter;
        if (Math.abs(Math.hypot(x,y) - customRingRadius) > 23) { custom.previous = null; return; }
        const minute = (Math.round((Math.atan2(y,x) + Math.PI / 2 + Math.PI * 2) / (Math.PI * 2) * 1440)) % 1440;
        for (const selected of customMinutesOnArc([...this.customButtons.keys()], custom.previous ?? minute, minute)) custom.selected.add(selected);
        custom.previous = minute;
        this.paintCustomGesture();
        return;
      }
      if (!this.gesture || this.gesture.id !== event.pointerId) return;
      const bounds = host.getBoundingClientRect();
      const hour = hitHour(
        ((event.clientX - bounds.left) * dialSize) / bounds.width,
        ((event.clientY - bounds.top) * dialSize) / bounds.height,
        this.gesture.ring,
      );
      if (hour === null) {
        this.gesture.previous = null;
        return;
      }
      this.slots = paintHours(
        this.slots,
        this.gesture.ring,
        this.gesture.previous ?? hour,
        hour,
        this.gesture.enabled,
      );
      this.gesture.previous = hour;
      this.paint();
    });
    const end = (event: PointerEvent, cancel: boolean): void => {
      const custom = this.customGesture;
      if (custom && custom.id === event.pointerId) {
        this.customGesture = null;
        if (host.hasPointerCapture(event.pointerId)) host.releasePointerCapture(event.pointerId);
        this.setCustomChimes(this.customChimes);
        if (!cancel) void this.setCustom([...custom.selected], custom.enabled);
        return;
      }
      if (!this.gesture || this.gesture.id !== event.pointerId) return;
      if (cancel) this.slots = this.gesture.original;
      this.gesture = null;
      if (host.hasPointerCapture(event.pointerId))
        host.releasePointerCapture(event.pointerId);
      this.paint();
      if (!cancel) this.commit([...this.slots]);
    };
    host.addEventListener('pointerup', (event) => end(event, false));
    host.addEventListener('pointercancel', (event) => end(event, true));
    host.addEventListener('lostpointercapture', (event) => {
      if (this.gesture || this.customGesture) end(event, true);
    });
  }
  private position(element: HTMLElement, hour: number, radius: number): void {
    const location = point(hour, radius);
    element.style.left = `${(location.x / dialSize) * 100}%`;
    element.style.top = `${(location.y / dialSize) * 100}%`;
  }
  setSlots(slots: readonly boolean[]): void {
    if (!this.gesture) {
      this.slots = [...slots];
      this.paint();
    }
  }
  private paintCustomGesture(): void {
    const gesture = this.customGesture;
    if (!gesture) return;
    this.renderCustomChimes(setCustomMinutes(gesture.original, [...gesture.selected], gesture.enabled));
  }
  setCustomChimes(chimes: readonly CustomChime[]): void {
    this.customChimes = chimes;
    if (!this.customGesture) this.renderCustomChimes(chimes);
  }
  private renderCustomChimes(chimes: readonly CustomChime[]): void {
    const minutes = customMinutes(chimes);
    this.customRing.style.display = chimes.length ? '' : 'none';
    this.host.dataset.customRing = String(chimes.length > 0);
    const registered = new Set(chimes.map(c => c.minute));
    for (const [minute, button] of this.customButtons) {
      if (!registered.has(minute)) {
        button.remove(); this.customButtons.delete(minute);
        this.customMarkers.get(minute)?.remove(); this.customMarkers.delete(minute);
      }
    }
    for (const minute of registered) {
      let button = this.customButtons.get(minute);
      if (!button) {
        button = document.createElement('button'); button.type = 'button'; button.className = 'custom-slot';
        button.dataset.minute = String(minute);
        const hour = minute / 60;
        button.dataset.side = hour >= 4 && hour <= 8 ? 'right' : hour >= 16 && hour <= 20 ? 'left' : '';
        this.position(button, hour, customRingRadius);
        const marker = document.createElementNS('http://www.w3.org/2000/svg','circle');
        const location = point(hour,customRingRadius);
        marker.setAttribute('cx',String(location.x)); marker.setAttribute('cy',String(location.y));
        marker.setAttribute('r','5.5'); marker.dataset.minute=String(minute);
        this.customMarkers.set(minute,marker); this.customRing.append(marker);
        button.addEventListener('click', () => {
          if (this.disabled || button!.dataset.busy === 'true') return;
          button!.dataset.busy = 'true';
          void this.setCustom([minute], button!.dataset.active !== 'true').finally(() => { button!.dataset.busy = 'false'; });
        });
        this.host.append(button); this.customButtons.set(minute, button);
      }
      button.disabled = this.disabled;
      button.setAttribute('aria-pressed', String(minutes.has(minute)));
      button.dataset.active = String(minutes.has(minute));
      this.customMarkers.get(minute)!.dataset.active=String(minutes.has(minute));
      button.dataset.tooltip = customMinuteTooltip(chimes, minute);
      button.setAttribute('aria-label', button.dataset.tooltip);
    }
  }
  setDisabled(disabled: boolean): void {
    this.disabled = disabled;
    for (const button of this.buttons) button.disabled = disabled;
    for (const button of this.customButtons.values()) button.disabled = disabled;
  }
  setAngle(angle: number): void {
    this.hand.setAttribute('transform', `rotate(${angle} ${dialCenter} ${dialCenter})`);
    this.hand.dataset.angle = String(angle);
  }
  private paint(): void {
    this.buttons.forEach((button, slot) => {
      button.setAttribute('aria-pressed', String(this.slots[slot]));
      button.dataset.active = String(this.slots[slot]);
      const description = slotTooltip(slot, this.slots[slot]);
      button.dataset.tooltip = description;
      button.setAttribute('aria-label', description);
    });
    this.preview(this.slots);
  }
}
