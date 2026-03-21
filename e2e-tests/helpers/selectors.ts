/** Centralized CSS selectors for E2E tests. Keep in sync with components.css. */

export const Nav = {
  link: 'a.nav-link',
  linkActive: 'a.nav-link.active',
  logout: 'button.btn-logout[title="Sign out"]',
} as const

export const Card = {
  container: 'div.card',
  title: 'span.card-title',
} as const

export const Table = {
  dataTable: 'table.data-table',
} as const

export const Status = {
  ok: 'span.tag-ok',
  warn: 'span.tag-warn',
  err: 'span.tag-err',
} as const

export const Toast = {
  container: 'div.toast-container',
  success: 'div.toast.toast-success',
  error: 'div.toast.toast-error',
} as const

export const Loading = {
  status: '[role="status"]',
  skeleton: 'div.skeleton',
} as const

export const Login = {
  submit: 'button.auth-submit',
  error: 'div.login-error',
  card: 'div.auth-card',
} as const

export const Form = {
  input: 'input.config-input',
  save: 'button.btn-save',
} as const

export const Config = {
  group: 'div.config-group',
  groupHeader: 'h3.config-group-header',
  label: 'label.config-label',
  saveBar: 'div.config-save-bar',
} as const

export const Kiro = {
  wrap: 'div.device-code-wrap',
} as const
