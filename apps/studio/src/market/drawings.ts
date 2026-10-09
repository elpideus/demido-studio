// What a TradingView script draws besides its plots (Pine's label.new, line.new and box.new),
// painted on the chart canvas as a lightweight-charts series primitive. Tables are HTML, laid over
// the pane by PriceChart.

import type {
  IChartApiBase,
  IPrimitivePaneRenderer,
  IPrimitivePaneView,
  ISeriesApi,
  ISeriesPrimitive,
  SeriesAttachedParameter,
  SeriesType,
  Time,
} from 'lightweight-charts';

import type { Bar, IndicatorBox, IndicatorGraphics, IndicatorLabel, IndicatorLine } from '@/lib/types';

type Target = Parameters<IPrimitivePaneRenderer['draw']>[0];
type Ctx = CanvasRenderingContext2D;

const FONT = "'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif";
const DEFAULT_COLOR = '#2962ff';

/** Pine's text sizes, in pixels. */
export function textSize(size: string): number {
  switch (size) {
    case 'tiny':
      return 8;
    case 'small':
      return 10;
    case 'large':
      return 15;
    case 'huge':
      return 20;
    default:
      return 11.5;
  }
}

function dash(ctx: Ctx, style: string, width: number): void {
  const w = Math.max(1, width);
  if (style === 'dotted') ctx.setLineDash([w, 2 * w]);
  else if (style === 'dashed') ctx.setLineDash([4 * w, 3 * w]);
  else ctx.setLineDash([]);
}

/** Where a line extended to `x` would be, on the line through two points. */
export function lineAt(x1: number, y1: number, x2: number, y2: number, x: number): number {
  if (x1 === x2) return y1;
  return y1 + ((y2 - y1) * (x - x1)) / (x2 - x1);
}

/** Draws one script's drawings on a pane; `barAt` gives the candle a label sits above or below. */
export class Drawings implements ISeriesPrimitive<Time> {
  private chart: IChartApiBase<Time> | null = null;
  private series: ISeriesApi<SeriesType, Time> | null = null;
  private request: (() => void) | null = null;
  private graphics: IndicatorGraphics | null = null;
  private visible = true;
  private readonly views: readonly IPrimitivePaneView[];

  constructor(private readonly barAt: (t: number) => Bar | undefined) {
    const renderer: IPrimitivePaneRenderer = { draw: (target) => this.draw(target) };
    this.views = [{ renderer: () => renderer }];
  }

  attached(param: SeriesAttachedParameter<Time, SeriesType>): void {
    this.chart = param.chart;
    this.series = param.series;
    this.request = param.requestUpdate;
  }

  detached(): void {
    this.chart = null;
    this.series = null;
    this.request = null;
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return this.views;
  }

  set(graphics: IndicatorGraphics | null): void {
    this.graphics = graphics;
    this.request?.();
  }

  setVisible(visible: boolean): void {
    this.visible = visible;
    this.request?.();
  }

  private draw(target: Target): void {
    const g = this.graphics;
    const chart = this.chart;
    const series = this.series;
    if (!g || !this.visible || !chart || !series) return;
    if (!g.boxes.length && !g.lines.length && !g.labels.length) return;
    target.useMediaCoordinateSpace(({ context: ctx, mediaSize }) => {
      const ts = chart.timeScale();
      const x = (t: number) => ts.timeToCoordinate(t as Time);
      const y = (p: number) => series.priceToCoordinate(p);
      const width = mediaSize.width;
      ctx.save();
      // Oldest first, so the newest drawings end up on top as in TradingView.
      for (let i = g.boxes.length - 1; i >= 0; i -= 1) drawBox(ctx, g.boxes[i]!, x, y, width);
      for (let i = g.lines.length - 1; i >= 0; i -= 1) drawLine(ctx, g.lines[i]!, x, y, width);
      for (let i = g.labels.length - 1; i >= 0; i -= 1) drawLabel(ctx, g.labels[i]!, x, y, this.barAt);
      ctx.restore();
    });
  }
}

type X = (t: number) => number | null;
type Y = (p: number) => number | null;

function drawBox(ctx: Ctx, b: IndicatorBox, x: X, y: Y, width: number): void {
  const left = b.extend === 'left' || b.extend === 'both' ? 0 : x(b.t1);
  const right = b.extend === 'right' || b.extend === 'both' ? width : x(b.t2);
  const top = y(b.y1);
  const bottom = y(b.y2);
  if (left === null || right === null || top === null || bottom === null) return;
  const w = right - left;
  const h = bottom - top;
  if (b.bg) {
    ctx.fillStyle = b.bg;
    ctx.fillRect(left, top, w, h);
  }
  if (b.color && b.width > 0) {
    ctx.strokeStyle = b.color;
    ctx.lineWidth = b.width;
    dash(ctx, b.style, b.width);
    ctx.strokeRect(left, top, w, h);
  }
  if (b.text) {
    const size = textSize(b.textSize);
    ctx.font = `${size}px ${FONT}`;
    ctx.fillStyle = b.textColor ?? '#d1d4dc';
    ctx.textAlign = b.halign === 'left' ? 'left' : b.halign === 'right' ? 'right' : 'center';
    ctx.textBaseline = b.valign === 'top' ? 'top' : b.valign === 'bottom' ? 'bottom' : 'middle';
    const tx = b.halign === 'left' ? left + 4 : b.halign === 'right' ? right - 4 : left + w / 2;
    const ty = b.valign === 'top' ? top + 3 : b.valign === 'bottom' ? bottom - 3 : top + h / 2;
    ctx.save();
    ctx.beginPath();
    ctx.rect(left, top, w, h);
    ctx.clip();
    drawLines(ctx, b.text, tx, ty, size, ctx.textBaseline);
    ctx.restore();
  }
}

function drawLine(ctx: Ctx, l: IndicatorLine, x: X, y: Y, width: number): void {
  let x1 = x(l.t1);
  let x2 = x(l.t2);
  let y1 = y(l.y1);
  let y2 = y(l.y2);
  if (x1 === null || x2 === null || y1 === null || y2 === null) return;
  const [ax, ay, bx, by] = [x1, y1, x2, y2];
  if (l.extend === 'left' || l.extend === 'both') {
    const edge = ax <= bx ? 0 : width;
    if (ax <= bx) [x1, y1] = [edge, lineAt(ax, ay, bx, by, edge)];
    else [x2, y2] = [edge, lineAt(ax, ay, bx, by, edge)];
  }
  if (l.extend === 'right' || l.extend === 'both') {
    const edge = ax <= bx ? width : 0;
    if (ax <= bx) [x2, y2] = [edge, lineAt(ax, ay, bx, by, edge)];
    else [x1, y1] = [edge, lineAt(ax, ay, bx, by, edge)];
  }
  ctx.strokeStyle = l.color ?? DEFAULT_COLOR;
  ctx.lineWidth = Math.max(1, l.width);
  dash(ctx, l.style, l.width);
  ctx.beginPath();
  ctx.moveTo(x1, y1);
  ctx.lineTo(x2, y2);
  ctx.stroke();
  ctx.setLineDash([]);
  if (l.style === 'arrow_right' || l.style === 'arrow_both') arrowHead(ctx, x1, y1, x2, y2, l.width);
  if (l.style === 'arrow_left' || l.style === 'arrow_both') arrowHead(ctx, x2, y2, x1, y1, l.width);
}

function arrowHead(ctx: Ctx, fromX: number, fromY: number, toX: number, toY: number, width: number): void {
  const angle = Math.atan2(toY - fromY, toX - fromX);
  const size = 6 + 2 * width;
  ctx.beginPath();
  ctx.moveTo(toX, toY);
  ctx.lineTo(toX - size * Math.cos(angle - Math.PI / 7), toY - size * Math.sin(angle - Math.PI / 7));
  ctx.moveTo(toX, toY);
  ctx.lineTo(toX - size * Math.cos(angle + Math.PI / 7), toY - size * Math.sin(angle + Math.PI / 7));
  ctx.stroke();
}

function drawLines(ctx: Ctx, text: string, x: number, y: number, size: number, baseline: CanvasTextBaseline): void {
  const lines = text.split('\n');
  const step = size * 1.25;
  const first =
    baseline === 'top'
      ? y
      : baseline === 'bottom'
        ? y - step * (lines.length - 1)
        : y - (step * (lines.length - 1)) / 2;
  lines.forEach((line, i) => ctx.fillText(line, x, first + i * step));
}

/** Gap between a bar's high or low and a label hung on it. */
const BAR_GAP = 6;

function drawLabel(ctx: Ctx, l: IndicatorLabel, x: X, y: Y, barAt: (t: number) => Bar | undefined): void {
  const px = x(l.t);
  if (px === null) return;
  let py: number | null = null;
  let style = l.style;
  if (l.yloc === 'price') {
    py = l.y === null ? null : y(l.y);
  } else {
    const bar = barAt(l.t);
    if (!bar) return;
    const at = y(l.yloc === 'abovebar' ? bar.h : bar.l);
    if (at !== null) py = l.yloc === 'abovebar' ? at - BAR_GAP : at + BAR_GAP;
    // Above a bar a label points down at it, below a bar up.
    if (style.startsWith('label_')) style = l.yloc === 'abovebar' ? 'label_down' : 'label_up';
  }
  if (py === null) return;
  const size = textSize(l.size);
  ctx.font = `${size}px ${FONT}`;
  const color = l.color ?? DEFAULT_COLOR;
  const lines = l.text ? l.text.split('\n') : [];
  const textW = lines.reduce((w, line) => Math.max(w, ctx.measureText(line).width), 0);
  const lineH = size * 1.25;
  const textH = lines.length * lineH;

  if (style === 'none') {
    if (!lines.length) return;
    ctx.fillStyle = l.textColor ?? color;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    drawLines(ctx, l.text, px, py, size, 'middle');
    return;
  }

  if (style.startsWith('label_')) {
    const padX = 6;
    const padY = 3;
    const w = Math.max(textW + 2 * padX, 12);
    const h = Math.max(textH + 2 * padY, 12);
    const tip = 5;
    // The bubble's top-left corner, with the pointer between it and the point.
    let bx = px - w / 2;
    let by = py - h / 2;
    if (style === 'label_down' || style === 'label_lower_left' || style === 'label_lower_right') by = py - tip - h;
    else if (style === 'label_up' || style === 'label_upper_left' || style === 'label_upper_right') by = py + tip;
    else if (style === 'label_left') bx = px + tip;
    else if (style === 'label_right') bx = px - tip - w;
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.roundRect(bx, by, w, h, 3);
    ctx.fill();
    ctx.beginPath();
    if (by + h <= py) {
      ctx.moveTo(px - tip, by + h);
      ctx.lineTo(px, py);
      ctx.lineTo(px + tip, by + h);
    } else if (by >= py) {
      ctx.moveTo(px - tip, by);
      ctx.lineTo(px, py);
      ctx.lineTo(px + tip, by);
    } else if (bx >= px) {
      ctx.moveTo(bx, py - tip);
      ctx.lineTo(px, py);
      ctx.lineTo(bx, py + tip);
    } else if (bx + w <= px) {
      ctx.moveTo(bx + w, py - tip);
      ctx.lineTo(px, py);
      ctx.lineTo(bx + w, py + tip);
    }
    ctx.fill();
    if (lines.length) {
      ctx.fillStyle = l.textColor ?? '#ffffff';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      drawLines(ctx, l.text, bx + w / 2, by + h / 2, size, 'middle');
    }
    return;
  }

  // A shape at the point, with its text beside it (below an up shape, above a down one).
  const r = Math.max(3, size / 2.4);
  ctx.fillStyle = color;
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  switch (style) {
    case 'triangleup':
    case 'arrowup':
      ctx.moveTo(px, py - r);
      ctx.lineTo(px + r, py + r);
      ctx.lineTo(px - r, py + r);
      ctx.closePath();
      ctx.fill();
      break;
    case 'triangledown':
    case 'arrowdown':
      ctx.moveTo(px, py + r);
      ctx.lineTo(px + r, py - r);
      ctx.lineTo(px - r, py - r);
      ctx.closePath();
      ctx.fill();
      break;
    case 'circle':
      ctx.arc(px, py, r, 0, 2 * Math.PI);
      ctx.fill();
      break;
    case 'diamond':
      ctx.moveTo(px, py - r);
      ctx.lineTo(px + r, py);
      ctx.lineTo(px, py + r);
      ctx.lineTo(px - r, py);
      ctx.closePath();
      ctx.fill();
      break;
    case 'cross':
      ctx.moveTo(px - r, py);
      ctx.lineTo(px + r, py);
      ctx.moveTo(px, py - r);
      ctx.lineTo(px, py + r);
      ctx.stroke();
      break;
    case 'xcross':
      ctx.moveTo(px - r, py - r);
      ctx.lineTo(px + r, py + r);
      ctx.moveTo(px + r, py - r);
      ctx.lineTo(px - r, py + r);
      ctx.stroke();
      break;
    case 'flag':
      ctx.moveTo(px - r, py + r);
      ctx.lineTo(px - r, py - r);
      ctx.lineTo(px + r, py - r / 3);
      ctx.lineTo(px - r, py + r / 3);
      ctx.stroke();
      ctx.fill();
      break;
    default:
      ctx.fillRect(px - r, py - r, 2 * r, 2 * r);
  }
  if (lines.length) {
    const below = style !== 'triangledown' && style !== 'arrowdown';
    ctx.fillStyle = l.textColor ?? color;
    ctx.textAlign = 'center';
    ctx.textBaseline = below ? 'top' : 'bottom';
    drawLines(ctx, l.text, px, below ? py + r + 3 : py - r - 3, size, ctx.textBaseline);
  }
}
