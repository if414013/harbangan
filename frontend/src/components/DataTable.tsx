import { useState, useMemo } from "react";
import type { ReactNode } from "react";
import { EmptyState } from "./EmptyState";

interface Column<T> {
  key: string;
  label: string;
  sortable?: boolean;
  render?: (row: T) => ReactNode;
}

interface DataTableProps<T> {
  data: T[];
  columns: Column<T>[];
  searchKeys?: string[];
  searchPlaceholder?: string;
  emptyTitle?: string;
  emptyDescription?: string;
  caption?: string;
}

export function DataTable<T extends Record<string, unknown>>({
  data,
  columns,
  searchKeys = [],
  searchPlaceholder = "Search...",
  emptyTitle = "No data",
  emptyDescription,
  caption,
}: DataTableProps<T>) {
  const [search, setSearch] = useState("");
  const [sortKey, setSortKey] = useState<string | null>(null);
  const [sortDir, setSortDir] = useState<"asc" | "desc">("asc");

  const filtered = useMemo(() => {
    if (!search.trim() || searchKeys.length === 0) return data;
    const q = search.toLowerCase();
    return data.filter((row) =>
      searchKeys.some((k) => {
        const val = row[k];
        return val != null && String(val).toLowerCase().includes(q);
      }),
    );
  }, [data, search, searchKeys]);

  const sorted = useMemo(() => {
    if (!sortKey) return filtered;
    return [...filtered].sort((a, b) => {
      const aVal = a[sortKey];
      const bVal = b[sortKey];
      if (aVal == null && bVal == null) return 0;
      if (aVal == null) return 1;
      if (bVal == null) return -1;
      if (typeof aVal === "number" && typeof bVal === "number") {
        return sortDir === "asc" ? aVal - bVal : bVal - aVal;
      }
      const cmp = String(aVal).localeCompare(String(bVal));
      return sortDir === "asc" ? cmp : -cmp;
    });
  }, [filtered, sortKey, sortDir]);

  function handleSort(key: string) {
    if (sortKey === key) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(key);
      setSortDir("asc");
    }
  }

  return (
    <div className="data-table-wrapper">
      {searchKeys.length > 0 && (
        <div className="data-table-toolbar">
          <input
            className="data-table-search"
            type="text"
            placeholder={searchPlaceholder}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label="Search table"
          />
          <span className="data-table-count">
            {filtered.length === data.length
              ? `${data.length} rows`
              : `${filtered.length} of ${data.length}`}
          </span>
        </div>
      )}
      <div className="data-table-scroll">
        {sorted.length === 0 ? (
          <EmptyState title={emptyTitle} description={emptyDescription} />
        ) : (
          <table className="data-table">
            {caption && <caption className="sr-only">{caption}</caption>}
            <thead>
              <tr>
                {columns.map((col) => (
                  <th
                    key={col.key}
                    className={col.sortable ? "sortable" : undefined}
                    onClick={
                      col.sortable ? () => handleSort(col.key) : undefined
                    }
                    aria-sort={
                      sortKey === col.key
                        ? sortDir === "asc"
                          ? "ascending"
                          : "descending"
                        : undefined
                    }
                  >
                    {col.label}
                    {col.sortable && sortKey === col.key && (
                      <span className="sort-indicator" aria-hidden="true">
                        {sortDir === "asc" ? " \u25B2" : " \u25BC"}
                      </span>
                    )}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {sorted.map((row, i) => (
                <tr key={i}>
                  {columns.map((col) => (
                    <td key={col.key}>
                      {col.render
                        ? col.render(row)
                        : String(row[col.key] ?? "\u2014")}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
