import { useState, useEffect, useMemo } from "react";
import { DataTable } from "../components/DataTable";
import { PageHeader } from "../components/PageHeader";
import { useSession } from "../components/SessionGate";
import { useToast } from "../components/useToast";
import {
  fetchUsage,
  fetchAdminUsage,
  fetchAdminUsageByUsers,
  type UsageRecord,
  type UserUsageRecord,
} from "../lib/api_usage";
import { formatCost, formatTokens } from "../lib/format";

type GroupBy = "day" | "model" | "provider";
type AdminTab = "my" | "global" | "users";

function getDefaultDateRange(): { start: string; end: string } {
  const end = new Date();
  const start = new Date();
  start.setDate(start.getDate() - 30);
  return {
    start: start.toISOString().split("T")[0],
    end: end.toISOString().split("T")[0],
  };
}

function calculateTotals(records: UsageRecord[]) {
  return records.reduce(
    (acc, r) => ({
      requests: acc.requests + r.request_count,
      inputTokens: acc.inputTokens + r.total_input_tokens,
      outputTokens: acc.outputTokens + r.total_output_tokens,
      cost: acc.cost + r.total_cost,
    }),
    { requests: 0, inputTokens: 0, outputTokens: 0, cost: 0 },
  );
}

function calculateUserTotals(records: UserUsageRecord[]) {
  return records.reduce(
    (acc, r) => ({
      requests: acc.requests + r.request_count,
      inputTokens: acc.inputTokens + r.total_input_tokens,
      outputTokens: acc.outputTokens + r.total_output_tokens,
      cost: acc.cost + r.total_cost,
    }),
    { requests: 0, inputTokens: 0, outputTokens: 0, cost: 0 },
  );
}

interface SummaryCardProps {
  label: string;
  value: string;
}

function SummaryCard({ label, value }: SummaryCardProps) {
  return (
    <div className="card">
      <div
        style={{
          fontSize: "0.65rem",
          textTransform: "uppercase",
          letterSpacing: "0.06em",
          color: "var(--text-tertiary)",
          marginBottom: 8,
          fontFamily: "var(--font-mono)",
        }}
      >
        {label}
      </div>
      <div
        style={{
          fontSize: "1.1rem",
          fontWeight: 600,
          color: "var(--green)",
          fontFamily: "var(--font-mono)",
          textShadow: "var(--glow-sm)",
        }}
      >
        {value}
      </div>
    </div>
  );
}

export function Usage() {
  const { user } = useSession();
  const { showToast } = useToast();
  const isAdmin = user.role === "admin";

  const defaultDates = useMemo(() => getDefaultDateRange(), []);
  const [startDate, setStartDate] = useState(defaultDates.start);
  const [endDate, setEndDate] = useState(defaultDates.end);
  const [groupBy, setGroupBy] = useState<GroupBy>("day");
  const [adminTab, setAdminTab] = useState<AdminTab>("my");

  const [myRecords, setMyRecords] = useState<UsageRecord[]>([]);
  const [globalRecords, setGlobalRecords] = useState<UsageRecord[]>([]);
  const [userRecords, setUserRecords] = useState<UserUsageRecord[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    async function loadData() {
      setLoading(true);
      try {
        const params = {
          start_date: startDate,
          end_date: endDate,
          group_by: groupBy,
        };

        if (!isAdmin || adminTab === "my") {
          const data = await fetchUsage(params);
          setMyRecords(data);
        } else if (adminTab === "global") {
          const data = await fetchAdminUsage(params);
          setGlobalRecords(data);
        } else if (adminTab === "users") {
          const data = await fetchAdminUsageByUsers({
            start_date: startDate,
            end_date: endDate,
          });
          setUserRecords(data);
        }
      } catch (err) {
        showToast(
          err instanceof Error ? err.message : "Failed to load usage data",
          "error",
        );
      } finally {
        setLoading(false);
      }
    }

    loadData();
  }, [startDate, endDate, groupBy, adminTab, isAdmin, showToast]);

  const totals = useMemo(() => {
    if (!isAdmin || adminTab === "my") {
      return calculateTotals(myRecords);
    } else if (adminTab === "global") {
      return calculateTotals(globalRecords);
    } else {
      return calculateUserTotals(userRecords);
    }
  }, [myRecords, globalRecords, userRecords, isAdmin, adminTab]);

  const currentRecords =
    !isAdmin || adminTab === "my"
      ? myRecords
      : adminTab === "global"
        ? globalRecords
        : [];

  return (
    <>
      <PageHeader
        title="usage"
        description="Request volume and token consumption across your API keys."
      />

      {isAdmin && (
        <div
          style={{
            display: "flex",
            gap: 4,
            marginBottom: 20,
            borderBottom: "1px solid var(--border)",
            paddingBottom: 8,
          }}
        >
          <button
            className={`btn-save${adminTab === "my" ? "" : " auth-submit-secondary"}`}
            onClick={() => setAdminTab("my")}
            type="button"
          >
            My Usage
          </button>
          <button
            className={`btn-save${adminTab === "global" ? "" : " auth-submit-secondary"}`}
            onClick={() => setAdminTab("global")}
            type="button"
          >
            Global
          </button>
          <button
            className={`btn-save${adminTab === "users" ? "" : " auth-submit-secondary"}`}
            onClick={() => setAdminTab("users")}
            type="button"
          >
            Per-User
          </button>
        </div>
      )}

      <div
        style={{
          display: "flex",
          gap: 12,
          marginBottom: 20,
          alignItems: "center",
          flexWrap: "wrap",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span
            style={{
              fontSize: "0.72rem",
              color: "var(--text-secondary)",
              fontFamily: "var(--font-mono)",
            }}
          >
            From:
          </span>
          <input
            type="date"
            className="config-input"
            value={startDate}
            onChange={(e) => setStartDate(e.target.value)}
            style={{ maxWidth: 140 }}
          />
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span
            style={{
              fontSize: "0.72rem",
              color: "var(--text-secondary)",
              fontFamily: "var(--font-mono)",
            }}
          >
            To:
          </span>
          <input
            type="date"
            className="config-input"
            value={endDate}
            onChange={(e) => setEndDate(e.target.value)}
            style={{ maxWidth: 140 }}
          />
        </div>
        {adminTab !== "users" && (
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span
              style={{
                fontSize: "0.72rem",
                color: "var(--text-secondary)",
                fontFamily: "var(--font-mono)",
              }}
            >
              Group by:
            </span>
            <select
              className="config-input"
              value={groupBy}
              onChange={(e) => setGroupBy(e.target.value as GroupBy)}
              style={{ maxWidth: 120 }}
            >
              <option value="day">day</option>
              <option value="model">model</option>
              <option value="provider">provider</option>
            </select>
          </div>
        )}
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))",
          gap: 12,
          marginBottom: 24,
        }}
      >
        <SummaryCard
          label="Total Requests"
          value={formatTokens(totals.requests)}
        />
        <SummaryCard
          label="Input Tokens"
          value={formatTokens(totals.inputTokens)}
        />
        <SummaryCard
          label="Output Tokens"
          value={formatTokens(totals.outputTokens)}
        />
        <SummaryCard label="Total Cost" value={formatCost(totals.cost)} />
      </div>

      <h2 className="section-header">
        {adminTab === "users" ? "USER BREAKDOWN" : "BREAKDOWN"}
      </h2>
      <div className="card">
        {loading ? (
          <div
            style={{
              padding: 40,
              textAlign: "center",
              color: "var(--text-tertiary)",
              fontFamily: "var(--font-mono)",
              fontSize: "0.78rem",
            }}
          >
            Loading...
          </div>
        ) : adminTab === "users" ? (
          <DataTable
            data={userRecords as unknown as Record<string, unknown>[]}
            columns={[
              {
                key: "email",
                label: "User",
                sortable: true,
              },
              {
                key: "total_input_tokens",
                label: "Input Tokens",
                sortable: true,
                render: (row) => formatTokens(row.total_input_tokens as number),
              },
              {
                key: "total_output_tokens",
                label: "Output Tokens",
                sortable: true,
                render: (row) =>
                  formatTokens(row.total_output_tokens as number),
              },
              {
                key: "total_cost",
                label: "Total Cost",
                sortable: true,
                render: (row) => formatCost(row.total_cost as number),
              },
              {
                key: "request_count",
                label: "Requests",
                sortable: true,
                render: (row) => formatTokens(row.request_count as number),
              },
            ]}
            searchKeys={["email"]}
            searchPlaceholder="Search users..."
            emptyTitle="No usage data"
            emptyDescription="No usage data found for the selected date range."
            caption="User usage breakdown"
          />
        ) : (
          <DataTable
            data={currentRecords as unknown as Record<string, unknown>[]}
            columns={[
              {
                key: "group_key",
                label:
                  groupBy === "day"
                    ? "Date"
                    : groupBy === "model"
                      ? "Model"
                      : "Provider",
                sortable: true,
              },
              {
                key: "total_input_tokens",
                label: "Input Tokens",
                sortable: true,
                render: (row) => formatTokens(row.total_input_tokens as number),
              },
              {
                key: "total_output_tokens",
                label: "Output Tokens",
                sortable: true,
                render: (row) =>
                  formatTokens(row.total_output_tokens as number),
              },
              {
                key: "total_cost",
                label: "Total Cost",
                sortable: true,
                render: (row) => formatCost(row.total_cost as number),
              },
              {
                key: "request_count",
                label: "Requests",
                sortable: true,
                render: (row) => formatTokens(row.request_count as number),
              },
            ]}
            searchKeys={["group_key"]}
            searchPlaceholder="Search..."
            emptyTitle="No usage data"
            emptyDescription="No usage data found for the selected date range."
            caption="Usage breakdown"
          />
        )}
      </div>
    </>
  );
}
