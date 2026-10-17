import {
  dialSize,
  dialCenter,
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
export class ClockDial {
  private slots = Array<boolean>(48).fill(false);
  private readonly buttons: HTMLButtonElement[] = [];
  private gesture: Gesture | null = null;
  private readonly hand: SVGLineElement;
  constructor(
    private readonly host: HTMLElement,
    private readonly commit: (slots: boolean[]) => void,
    private readonly preview: (slots: readonly boolean[]) => void = () => {},
  ) {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('viewBox', `0 0 ${dialSize} ${dialSize}`);
    svg.setAttribute('aria-hidden', 'true');
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
        '.slot',
      );
      if (!button || button.disabled || this.gesture || event.button !== 0)
        return;
      event.preventDefault();
      button.focus({ preventScroll: true });
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
      if (this.gesture) end(event, true);
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
  setDisabled(disabled: boolean): void {
    for (const button of this.buttons) button.disabled = disabled;
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
