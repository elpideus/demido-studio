import type { CSSProperties } from 'react';

import type { IndicatorTable } from '@/lib/types';
import { textSize } from './drawings';
import { tableGrid, tablePlacement } from './indicatorView';
import styles from './MarketWindow.module.css';

const ALIGN: Record<string, CSSProperties['textAlign']> = { left: 'left', right: 'right', center: 'center' };
const VALIGN: Record<string, CSSProperties['verticalAlign']> = { top: 'top', bottom: 'bottom', center: 'middle' };

/** A Pine `table.new`, where the script placed it in its pane. */
export function ScriptTable({ table }: { table: IndicatorTable }) {
  const grid = tableGrid(table.rows, table.columns, table.cells);
  if (!table.cells.length) return null;
  const border = table.border && table.borderWidth > 0 ? `${table.borderWidth}px solid ${table.border}` : undefined;
  return (
    <table
      className={styles.scriptTable}
      style={{
        ...tablePlacement(table.position),
        background: table.bg ?? undefined,
        outline: table.frame && table.frameWidth > 0 ? `${table.frameWidth}px solid ${table.frame}` : undefined,
      }}
    >
      <tbody>
        {grid.map((line, r) => (
          <tr key={r}>
            {line.map(({ col, cell }) => (
              <td
                key={col}
                colSpan={cell && cell.colspan > 1 ? cell.colspan : undefined}
                rowSpan={cell && cell.rowspan > 1 ? cell.rowspan : undefined}
                title={cell?.tooltip}
                style={{
                  border,
                  background: cell?.bg ?? undefined,
                  color: cell?.textColor ?? undefined,
                  fontSize: cell ? textSize(cell.size) : undefined,
                  textAlign: cell ? (ALIGN[cell.halign] ?? 'center') : undefined,
                  verticalAlign: cell ? (VALIGN[cell.valign] ?? 'middle') : undefined,
                  pointerEvents: cell?.tooltip ? 'auto' : undefined,
                }}
              >
                {cell?.text}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
