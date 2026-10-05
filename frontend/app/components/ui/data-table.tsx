import type { HTMLAttributes, KeyboardEvent, ReactNode } from "react";
import {
  createColumnHelper,
  createSortedRowModel,
  metaHelper,
  rowSelectionFeature,
  rowSortingFeature,
  sortFn_alphanumeric,
  sortFn_basic,
  tableFeatures,
  useTable,
  type ColumnDef,
  type RowData,
  type RowSelectionState,
  type SortingState,
} from "@tanstack/react-table";
import { LuArrowDown, LuArrowUp, LuArrowUpDown } from "react-icons/lu";

import { Checkbox } from "./checkbox";
import { focusRing } from "./styles";

type ColumnMeta = {
  align?: "right";
  // extra classes for the column's header and cells, e.g. to hide it on small screens
  className?: string;
};

const features = tableFeatures({
  rowSelectionFeature,
  rowSortingFeature,
  sortedRowModel: createSortedRowModel(),
  sortFns: { alphanumeric: sortFn_alphanumeric, basic: sortFn_basic },
  columnMeta: metaHelper<ColumnMeta>(),
});

type Features = typeof features;
// columns hold different value types, so the value type is left open
export type Column<T extends RowData> = ColumnDef<Features, T, any>;
export type { RowSelectionState, SortingState };
export const columnsFor = <T extends RowData>() => createColumnHelper<Features, T>();

type DataTableProps<T extends RowData> = {
  label: string;
  data: T[];
  columns: Column<T>[];
  rowId: (row: T) => string;
  onRowClick?: (row: T) => void;
  empty?: ReactNode;
  // a list sorted by the server passes its sort, and hears when a header asks for another;
  // without it the table sorts the rows it has
  sorting?: SortingState;
  onSortingChange?: (sorting: SortingState) => void;
  // passing a selection adds a checkbox to every row that `canSelect` allows
  selection?: RowSelectionState;
  onSelectionChange?: (selection: RowSelectionState) => void;
  canSelect?: (row: T) => boolean;
  // how screen readers name a row's checkbox, e.g. `Select ada@example.com`
  rowLabel?: (row: T) => string;
  // extra attributes per row, e.g. to drag rows or drop onto them; the row's id is in `data-row-id`
  rowProps?: (row: T) => HTMLAttributes<HTMLTableRowElement>;
  // replaces the default space above the table, e.g. inside a section that already spaces its content
  className?: string;
};

const SORT_ICON = { asc: LuArrowUp, desc: LuArrowDown };

export function DataTable<T extends RowData>({ label, data, columns, rowId, onRowClick, empty = "Nothing here yet.", sorting, onSortingChange, selection, onSelectionChange, canSelect, rowLabel, rowProps, className = "mt-4" }: DataTableProps<T>) {
  const controlled = sorting !== undefined;
  const selectable = selection !== undefined;
  const table = useTable({
    features,
    data,
    columns,
    getRowId: (row) => rowId(row),
    manualSorting: controlled,
    enableSortingRemoval: !controlled,
    enableRowSelection: (row) => selectable && (canSelect?.(row.original) ?? true),
    state: { ...(controlled && { sorting }), ...(selectable && { rowSelection: selection }) },
    ...(controlled && {
      onSortingChange: (updater) => onSortingChange?.(typeof updater === "function" ? updater(sorting) : updater),
    }),
    ...(selectable && {
      onRowSelectionChange: (updater) => onSelectionChange?.(typeof updater === "function" ? updater(selection) : updater),
    }),
  });
  const rows = table.getRowModel().rows;

  function onRowKey(event: KeyboardEvent<HTMLTableRowElement>, row: T) {
    if (event.target !== event.currentTarget || (event.key !== "Enter" && event.key !== " ")) return;
    event.preventDefault();
    onRowClick?.(row);
  }

  return (
    <div className={`ui-card overflow-x-auto ${className}`}>
      <table aria-label={label} className="w-full text-left text-sm">
        <thead className="border-b border-line text-xs text-muted">
          {table.getHeaderGroups().map((group) => (
            <tr key={group.id}>
              {selectable && (
                <th scope="col" className="w-0 py-2.5 pr-0 pl-4">
                  <Checkbox
                    label="Select all"
                    checked={table.getIsAllRowsSelected()}
                    indeterminate={table.getIsSomeRowsSelected()}
                    disabled={!rows.some((row) => row.getCanSelect())}
                    onCheckedChange={(checked) => table.toggleAllRowsSelected(checked)}
                  />
                </th>
              )}
              {group.headers.map((header) => {
                const meta = header.column.columnDef.meta;
                const sorted = header.column.getIsSorted();
                const Icon = sorted ? SORT_ICON[sorted] : LuArrowUpDown;
                const align = meta?.align === "right" ? "text-right" : "";
                return (
                  <th
                    key={header.id}
                    scope="col"
                    aria-sort={sorted === "asc" ? "ascending" : sorted === "desc" ? "descending" : undefined}
                    className={`px-4 py-2.5 font-medium whitespace-nowrap ${align} ${meta?.className ?? ""}`}
                  >
                    {header.isPlaceholder ? null : header.column.getCanSort() ? (
                      <button
                        type="button"
                        onClick={header.column.getToggleSortingHandler()}
                        className={`group/sort inline-flex cursor-pointer items-center gap-1 rounded hover:text-ink ${meta?.align === "right" ? "flex-row-reverse" : ""} ${sorted ? "text-ink" : ""} ${focusRing}`}
                      >
                        <table.FlexRender header={header} />
                        <Icon size={12} className={sorted ? "" : "opacity-0 transition-opacity group-hover/sort:opacity-60 pointer-coarse:opacity-40"} />
                      </button>
                    ) : (
                      <table.FlexRender header={header} />
                    )}
                  </th>
                );
              })}
            </tr>
          ))}
        </thead>
        <tbody>
          {rows.map((row) => {
            const extra = rowProps?.(row.original);
            return (
              <tr
                key={row.id}
                {...extra}
                data-row-id={row.id}
                {...(onRowClick && {
                  tabIndex: 0,
                  "aria-haspopup": "dialog" as const,
                  onClick: () => onRowClick(row.original),
                  onKeyDown: (event: KeyboardEvent<HTMLTableRowElement>) => onRowKey(event, row.original),
                })}
                className={`border-b border-line last:border-0 ${row.getIsSelected() ? "bg-subtle/40" : ""} ${onRowClick ? `cursor-pointer transition-colors hover:bg-subtle/60 ${focusRing}` : ""} ${extra?.className ?? ""}`}
              >
                {selectable && (
                  <td className="w-0 py-3 pr-0 pl-4" onClick={(event) => event.stopPropagation()}>
                    <Checkbox label={`Select ${rowLabel?.(row.original) ?? "row"}`} checked={row.getIsSelected()} disabled={!row.getCanSelect()} onCheckedChange={(checked) => row.toggleSelected(checked)} />
                  </td>
                )}
                {row.getAllCells().map((cell) => {
                  const meta = cell.column.columnDef.meta;
                  return (
                    <td key={cell.id} className={`px-4 py-3 ${meta?.align === "right" ? "text-right tabular-nums" : ""} ${meta?.className ?? ""}`}>
                      <table.FlexRender cell={cell} />
                    </td>
                  );
                })}
              </tr>
            );
          })}
          {rows.length === 0 && (
            <tr>
              <td colSpan={columns.length + (selectable ? 1 : 0)} className="px-4 py-10 text-center text-sm text-muted">
                {empty}
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
