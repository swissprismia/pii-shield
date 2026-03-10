import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';

// ── Application State ────────────────────────────────────────────────────────

let state = {
  originalText: '',
  tokenizedText: '',
  entities: [],
  tokenMap: {},
  stats: {
    scanned: 0,
    detected: 0,
    tokenized: 0,
    detokenized: 0,
  },
  history: [],
  config: null,
  sidecarReady: false,
};

// ── DOM Elements ─────────────────────────────────────────────────────────────

const elements = {
  // Dashboard
  loadingSection: document.getElementById('loading-section'),
  detectionSection: document.getElementById('detection-section'),
  emptySection: document.getElementById('empty-section'),
  detectionList: document.getElementById('detection-list'),
  detectionTitle: document.getElementById('detection-title'),
  originalText: document.getElementById('original-text'),
  tokenizedText: document.getElementById('tokenized-text'),
  tokenMapSection: document.getElementById('token-map-section'),
  vaultTokenMapSummary: document.getElementById('token-map-summary'),
  btnViewVault: document.getElementById('btn-view-vault'),
  piiCount: document.getElementById('pii-count'),
  btnTokenize: document.getElementById('btn-tokenize'),
  btnIgnore: document.getElementById('btn-ignore'),
  // Vault tab
  vaultList: document.getElementById('vault-list'),
  vaultEmpty: document.getElementById('vault-empty'),
  vaultCount: document.getElementById('vault-count'),
  vaultClearAll: document.getElementById('btn-vault-clear-all'),
  vaultPreview: document.getElementById('vault-preview'),
  vaultOriginalText: document.getElementById('vault-original-text'),
  vaultTokenizedText: document.getElementById('vault-tokenized-text'),
  // Stats
  statScanned: document.getElementById('stat-scanned'),
  statDetected: document.getElementById('stat-detected'),
  statTokenized: document.getElementById('stat-tokenized'),
  statDetokenized: document.getElementById('stat-detokenized'),
  // Header
  activeWindow: document.getElementById('active-window'),
  statusLabel: document.getElementById('status-label'),
  toastContainer: document.getElementById('toast-container'),
  // History
  historyList: document.getElementById('history-list'),
  btnClearHistory: document.getElementById('btn-clear-history'),
  // Settings
  settingLanguage: document.getElementById('setting-language'),
  settingThreshold: document.getElementById('setting-threshold'),
  thresholdDisplay: document.getElementById('threshold-display'),
  browsersInput: document.getElementById('browsers-input'),
  browsersAdd: document.getElementById('browsers-add'),
  browsersList: document.getElementById('browsers-list'),
  aiAssistantsInput: document.getElementById('ai-assistants-input'),
  aiAssistantsAdd: document.getElementById('ai-assistants-add'),
  aiAssistantsList: document.getElementById('ai-assistants-list'),
  customAppsInput: document.getElementById('custom-apps-input'),
  customAppsAdd: document.getElementById('custom-apps-add'),
  customAppsList: document.getElementById('custom-apps-list'),
  btnSettingsSave: document.getElementById('btn-settings-save'),
  btnSettingsReset: document.getElementById('btn-settings-reset'),
};

// ── Toast Notification System ─────────────────────────────────────────────────

const SECRET_ENTITY_TYPES = new Set([
  'API_KEY', 'OPENAI_API_KEY', 'ANTHROPIC_API_KEY',
  'AWS_ACCESS_KEY', 'GITHUB_TOKEN', 'JWT_TOKEN', 'PRIVATE_KEY',
]);

function showToast(title, message, type = 'info', duration = 4000, onClick = null) {
  const toast = document.createElement('div');
  toast.className = `toast ${type}`;

  const icons = {
    success: '✓',
    warning: '⚠',
    error: '✕',
    danger: '🔐',
    info: 'ℹ',
  };

  toast.innerHTML = `
    <span class="toast-icon">${icons[type] || icons.info}</span>
    <div class="toast-content">
      <div class="toast-title">${title}</div>
      ${message ? `<div class="toast-message">${message}</div>` : ''}
    </div>
    <button class="toast-close" title="Dismiss">✕</button>
  `;

  const closeBtn = toast.querySelector('.toast-close');
  closeBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    removeToast(toast);
  });

  if (onClick) {
    toast.style.cursor = 'pointer';
    toast.addEventListener('click', () => {
      onClick();
      removeToast(toast);
    });
  }

  // Stack: newest on bottom
  elements.toastContainer.appendChild(toast);

  const timer = setTimeout(() => removeToast(toast), duration);
  toast._timer = timer;

  return toast;
}

function removeToast(toast) {
  clearTimeout(toast._timer);
  toast.style.animation = 'slideOut 0.3s ease forwards';
  setTimeout(() => toast.remove(), 300);
}

// ── Entity Configuration ──────────────────────────────────────────────────────

const entityConfig = {
  // PII (orange palette)
  PERSON:          { label: 'Person',           color: '#f59e0b', secret: false },
  EMAIL_ADDRESS:   { label: 'Email',            color: '#ef4444', secret: false },
  PHONE_NUMBER:    { label: 'Phone',            color: '#8b5cf6', secret: false },
  CREDIT_CARD:     { label: 'Credit Card',      color: '#ec4899', secret: false },
  US_SSN:          { label: 'SSN',              color: '#ef4444', secret: false },
  IP_ADDRESS:      { label: 'IP Address',       color: '#06b6d4', secret: false },
  URL:             { label: 'URL',              color: '#3b82f6', secret: false },
  LOCATION:        { label: 'Location',         color: '#22c55e', secret: false },
  DATE_TIME:       { label: 'Date/Time',        color: '#a855f7', secret: false },
  DOMAIN_NAME:     { label: 'Domain',           color: '#6366f1', secret: false },
  IBAN_CODE:       { label: 'IBAN',             color: '#f43f5e', secret: false },
  US_BANK_NUMBER:  { label: 'Bank #',           color: '#f43f5e', secret: false },
  US_PASSPORT:     { label: 'Passport',         color: '#dc2626', secret: false },
  NRP:             { label: 'NRP',              color: '#f97316', secret: false },
  MEDICAL_LICENSE: { label: 'Medical License',  color: '#14b8a6', secret: false },
  SWISS_AVS_NUMBER:{ label: 'AVS/AHV',          color: '#fb923c', secret: false },
  // Secrets (red/danger)
  API_KEY:          { label: 'API Key',          color: '#ef4444', secret: true },
  OPENAI_API_KEY:   { label: 'OpenAI Key',       color: '#ef4444', secret: true },
  ANTHROPIC_API_KEY:{ label: 'Anthropic Key',    color: '#ef4444', secret: true },
  AWS_ACCESS_KEY:   { label: 'AWS Access Key',   color: '#dc2626', secret: true },
  GITHUB_TOKEN:     { label: 'GitHub Token',     color: '#dc2626', secret: true },
  JWT_TOKEN:        { label: 'JWT Token',        color: '#b91c1c', secret: true },
  PRIVATE_KEY:      { label: 'Private Key',      color: '#991b1b', secret: true },
};

// ── Token Prefix → Entity Type mapping ───────────────────────────────────────

const TOKEN_PREFIX_TO_ENTITY_TYPE = {
  // PII
  FirstName: 'PERSON', LastName: 'PERSON', Name: 'PERSON', MiddleName: 'PERSON',
  Email: 'EMAIL_ADDRESS', Phone: 'PHONE_NUMBER', CreditCard: 'CREDIT_CARD',
  SSN: 'US_SSN', IP: 'IP_ADDRESS', URL: 'URL', Location: 'LOCATION',
  Date: 'DATE_TIME', Domain: 'DOMAIN_NAME', IBAN: 'IBAN_CODE',
  BankAccount: 'US_BANK_NUMBER', Passport: 'US_PASSPORT', NRP: 'NRP',
  MedicalLicense: 'MEDICAL_LICENSE', AVS: 'SWISS_AVS_NUMBER',
  // Secrets
  APIKey: 'API_KEY', OpenAIKey: 'OPENAI_API_KEY', AnthropicKey: 'ANTHROPIC_API_KEY',
  AWSKey: 'AWS_ACCESS_KEY', GitHubToken: 'GITHUB_TOKEN', JWT: 'JWT_TOKEN',
  PrivKey: 'PRIVATE_KEY',
};

function getTokenEntityConfig(tokenId) {
  const base = tokenId.replace(/\d+$/, ''); // strip trailing digits
  const entityType = TOKEN_PREFIX_TO_ENTITY_TYPE[base];
  if (entityType && entityConfig[entityType]) return entityConfig[entityType];
  return { label: base, color: '#666', secret: false };
}

// ── Tab Navigation ────────────────────────────────────────────────────────────

function initTabs() {
  const tabBtns = document.querySelectorAll('.tab-btn');
  const tabPanels = document.querySelectorAll('.tab-panel');

  tabBtns.forEach((btn) => {
    btn.addEventListener('click', () => {
      const tab = btn.dataset.tab;
      tabBtns.forEach((b) => b.classList.remove('active'));
      tabPanels.forEach((p) => (p.style.display = 'none'));
      btn.classList.add('active');
      document.getElementById(`tab-${tab}`).style.display = 'flex';

      if (tab === 'settings') {
        renderSettings();
      } else if (tab === 'history') {
        renderHistory();
      } else if (tab === 'vault') {
        renderVault();
      }
    });
  });
}

// ── Dashboard UI ──────────────────────────────────────────────────────────────

function updateDashboard() {
  elements.statScanned.textContent = state.stats.scanned;
  elements.statDetected.textContent = state.stats.detected;
  elements.statTokenized.textContent = state.stats.tokenized;
  elements.statDetokenized.textContent = state.stats.detokenized;

  // Loading / Ready state
  if (!state.sidecarReady) {
    elements.loadingSection.style.display = 'block';
    elements.detectionSection.style.display = 'none';
    elements.emptySection.style.display = 'none';
    return;
  }

  elements.loadingSection.style.display = 'none';

  if (state.entities.length > 0) {
    elements.detectionSection.style.display = 'block';
    elements.emptySection.style.display = 'none';

    const hasSecrets = state.entities.some((e) => SECRET_ENTITY_TYPES.has(e.entity_type));
    elements.detectionTitle.textContent = hasSecrets ? 'Secrets & PII Detected' : 'PII Detected';
    elements.piiCount.textContent = state.entities.length;

    elements.detectionList.innerHTML = state.entities
      .map((entity) => {
        const config = entityConfig[entity.entity_type] || { label: entity.entity_type, color: '#666', secret: false };
        const truncated = entity.text.length > 30 ? entity.text.substring(0, 30) + '…' : entity.text;
        const confidence = Math.round(entity.score * 100);
        const secretClass = config.secret ? ' secret' : '';
        return `
          <div class="pii-tag${secretClass}" title="${confidence}% confidence">
            <span class="type" style="color: ${config.color}">${config.label}</span>
            <span class="value">${escapeHtml(truncated)}</span>
          </div>
        `;
      })
      .join('');

    elements.originalText.textContent = state.originalText;
    elements.tokenizedText.textContent = state.tokenizedText;

    updateDashboardVaultSummary();
  } else {
    elements.detectionSection.style.display = 'none';
    elements.emptySection.style.display = 'block';
    elements.tokenMapSection.style.display = 'none';
  }
}

function updateDashboardVaultSummary() {
  const count = Object.keys(state.tokenMap).length;
  if (count > 0) {
    elements.tokenMapSection.style.display = 'block';
    elements.vaultTokenMapSummary.textContent =
      `${count} token${count !== 1 ? 's' : ''} stored in vault`;
  } else {
    elements.tokenMapSection.style.display = 'none';
  }
}

function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

// ── History Tab ───────────────────────────────────────────────────────────────

function renderHistory() {
  if (state.history.length === 0) {
    elements.historyList.innerHTML = '<div class="empty-history">No activity yet this session.</div>';
    return;
  }

  elements.historyList.innerHTML = [...state.history]
    .reverse()
    .map((entry) => {
      const time = new Date(entry.timestamp * 1000).toLocaleTimeString();
      const actionColor = {
        detected: 'var(--warning)',
        tokenized: 'var(--accent)',
        detokenized: 'var(--success)',
      }[entry.action] || 'var(--text-muted)';

      return `
        <div class="history-entry action-${entry.action}">
          <div class="history-entry-header">
            <span class="history-action" style="color: ${actionColor}">${entry.action}</span>
            <span class="history-time">${time}</span>
          </div>
          <div class="history-preview">${escapeHtml(entry.original_preview)}</div>
          <div class="history-meta">${entry.entity_count} entit${entry.entity_count === 1 ? 'y' : 'ies'} · ${entry.app_name}</div>
        </div>
      `;
    })
    .join('');
}

// ── Vault Tab ─────────────────────────────────────────────────────────────────

function renderVault() {
  const entries = Object.entries(state.tokenMap);
  const count = entries.length;

  // Update count badge
  elements.vaultCount.textContent = count === 0 ? 'empty' : `${count} token${count !== 1 ? 's' : ''}`;

  // Show/hide Clear All button
  elements.vaultClearAll.style.display = count > 0 ? 'block' : 'none';

  // Show/hide text preview
  if (count > 0 && (state.originalText || state.tokenizedText)) {
    elements.vaultPreview.style.display = 'block';
    elements.vaultOriginalText.textContent = state.originalText;
    elements.vaultTokenizedText.textContent = state.tokenizedText;
  } else {
    elements.vaultPreview.style.display = 'none';
  }

  // Show/hide empty state
  elements.vaultEmpty.style.display = count === 0 ? 'block' : 'none';
  elements.vaultList.style.display = count === 0 ? 'none' : 'flex';

  if (count === 0) {
    elements.vaultList.innerHTML = '';
    return;
  }

  elements.vaultList.innerHTML = entries
    .map(([tokenId, value]) => {
      const cfg = getTokenEntityConfig(tokenId);
      const secretClass = cfg.secret ? ' vault-entry--secret' : '';
      const badgeClass = cfg.secret ? ' vault-badge--danger' : '';
      return `
        <div class="vault-entry${secretClass}" data-token-id="${escapeHtml(tokenId)}">
          <span class="vault-badge${badgeClass}" style="background: ${cfg.color}22; color: ${cfg.color}; border-color: ${cfg.color}44">${escapeHtml(cfg.label)}</span>
          <div class="vault-entry-body">
            <span class="vault-token-id">[${escapeHtml(tokenId)}]</span>
            <span class="vault-token-value">${escapeHtml(value)}</span>
          </div>
          <div class="vault-entry-actions">
            <button class="btn btn-small btn-secondary vault-copy-btn" data-value="${escapeHtml(value)}" title="Copy original value">⎘</button>
            <button class="vault-delete-btn" data-token-id="${escapeHtml(tokenId)}" title="Delete token">🗑</button>
          </div>
        </div>
      `;
    })
    .join('');

  // Wire copy buttons
  elements.vaultList.querySelectorAll('.vault-copy-btn').forEach((btn) => {
    btn.addEventListener('click', async () => {
      await writeText(btn.dataset.value);
      showToast('Copied', 'Original value copied to clipboard', 'info', 2000);
    });
  });

  // Wire delete buttons
  elements.vaultList.querySelectorAll('.vault-delete-btn').forEach((btn) => {
    btn.addEventListener('click', () => handleDeleteToken(btn.dataset.tokenId));
  });
}

async function handleDeleteToken(tokenId) {
  try {
    await invoke('delete_token', { tokenId });
    delete state.tokenMap[tokenId];
    renderVault();
    updateDashboardVaultSummary();
    showToast('Token Deleted', `[${tokenId}] removed from vault`, 'info', 2000);
  } catch (err) {
    showToast('Error', `Failed to delete token: ${err}`, 'error');
  }
}

// ── Settings Tab ──────────────────────────────────────────────────────────────

function renderChipList(containerId, items, onRemove) {
  const container = document.getElementById(containerId);
  if (!container) return;
  container.innerHTML = items
    .map((item) => `
      <span class="chip">
        ${escapeHtml(item)}
        <button class="chip-remove" data-value="${escapeHtml(item)}" title="Remove">×</button>
      </span>
    `)
    .join('');
  container.querySelectorAll('.chip-remove').forEach((btn) => {
    btn.addEventListener('click', () => onRemove(btn.dataset.value));
  });
}

function renderSettings() {
  if (!state.config) return;

  elements.settingLanguage.value = state.config.language || 'en';
  const threshold = state.config.score_threshold ?? 0.5;
  elements.settingThreshold.value = threshold;
  elements.thresholdDisplay.textContent = threshold.toFixed(2);

  const cfg = state.config.auto_anonymize;

  function makeRemover(listKey) {
    return (value) => {
      state.config.auto_anonymize[listKey] = state.config.auto_anonymize[listKey].filter((v) => v !== value);
      renderSettings();
    };
  }

  renderChipList('browsers-list', cfg.browsers, makeRemover('browsers'));
  renderChipList('ai-assistants-list', cfg.ai_assistants, makeRemover('ai_assistants'));
  renderChipList('custom-apps-list', cfg.custom_apps, makeRemover('custom_apps'));
}

function addChip(listKey, inputEl) {
  const value = inputEl.value.trim().toLowerCase();
  if (!value || !state.config) return;
  if (!state.config.auto_anonymize[listKey].includes(value)) {
    state.config.auto_anonymize[listKey].push(value);
    renderSettings();
  }
  inputEl.value = '';
}

async function saveSettings() {
  if (!state.config) return;
  state.config.language = elements.settingLanguage.value;
  state.config.score_threshold = parseFloat(elements.settingThreshold.value);

  try {
    await invoke('save_config', { newConfig: state.config });
    showToast('Settings Saved', 'Configuration updated successfully', 'success');
  } catch (err) {
    console.error('Failed to save settings:', err);
    showToast('Error', 'Failed to save settings', 'error');
  }
}

async function resetSettings() {
  try {
    state.config = await invoke('get_config');
    // Reset to loaded defaults (which come from Rust Default impl)
    renderSettings();
    showToast('Settings Reset', 'Defaults restored (not saved yet)', 'info');
  } catch (err) {
    console.error('Failed to reload config:', err);
  }
}

function initSettingsListeners() {
  elements.settingThreshold?.addEventListener('input', () => {
    elements.thresholdDisplay.textContent = parseFloat(elements.settingThreshold.value).toFixed(2);
  });

  elements.browsersAdd?.addEventListener('click', () => addChip('browsers', elements.browsersInput));
  elements.aiAssistantsAdd?.addEventListener('click', () => addChip('ai_assistants', elements.aiAssistantsInput));
  elements.customAppsAdd?.addEventListener('click', () => addChip('custom_apps', elements.customAppsInput));

  // Enter key on inputs
  [elements.browsersInput, elements.aiAssistantsInput, elements.customAppsInput].forEach((input, i) => {
    const keys = ['browsers', 'ai_assistants', 'custom_apps'];
    input?.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') addChip(keys[i], input);
    });
  });

  elements.btnSettingsSave?.addEventListener('click', saveSettings);
  elements.btnSettingsReset?.addEventListener('click', resetSettings);
}

// ── Dashboard Action Handlers ─────────────────────────────────────────────────

async function handleTokenize() {
  try {
    await writeText(state.tokenizedText);
    state.stats.tokenized++;
    state.originalText = '';
    state.tokenizedText = '';
    state.entities = [];
    updateDashboard();
    showToast('Tokenized', 'Tokenized text copied to clipboard', 'success');
    await invoke('mark_clipboard_handled');
  } catch (error) {
    console.error('Failed to copy tokenized text:', error);
    showToast('Error', 'Failed to copy to clipboard', 'error');
  }
}

async function handleIgnore() {
  state.originalText = '';
  state.tokenizedText = '';
  state.entities = [];
  state.tokenMap = {};
  updateDashboard();
  try {
    await invoke('mark_clipboard_handled');
    await invoke('clear_token_vault');
  } catch (error) {
    console.error('Failed to handle ignore:', error);
  }
}

async function handleClearVault() {
  try {
    await invoke('clear_token_vault');
    state.tokenMap = {};
    renderVault();
    updateDashboardVaultSummary();
    showToast('Vault Cleared', 'Token mappings have been cleared', 'info');
  } catch (error) {
    showToast('Error', 'Failed to clear token vault', 'error');
  }
}

function handleClearHistory() {
  state.history = [];
  renderHistory();
}

// ── Tauri Event Listeners ─────────────────────────────────────────────────────

async function initTauriListeners() {
  await listen('pii-detected', (event) => {
    const { original_text, tokenized_text, token_map, entities } = event.payload;

    state.originalText = original_text;
    state.tokenizedText = tokenized_text;
    state.tokenMap = token_map || {};
    state.entities = entities;
    state.stats.scanned++;
    state.stats.detected += entities.length;

    updateDashboard();

    const hasSecrets = entities.some((e) => SECRET_ENTITY_TYPES.has(e.entity_type));
    const toastType = hasSecrets ? 'danger' : 'warning';
    const toastTitle = hasSecrets
      ? `${entities.length} secret${entities.length > 1 ? 's' : ''} detected!`
      : `${entities.length} PII item${entities.length > 1 ? 's' : ''} detected`;

    showToast(toastTitle, 'Click to view and tokenize', toastType, 5000, () => {
      // Switch to dashboard tab
      document.querySelector('[data-tab="dashboard"]').click();
      elements.detectionSection.scrollIntoView({ behavior: 'smooth' });
    });
  });

  await listen('active-window-changed', (event) => {
    const { title, app_name } = event.payload;
    elements.activeWindow.textContent = app_name || title || '—';
  });

  await listen('clipboard-scanned', () => {
    state.stats.scanned++;
    updateDashboard();
  });

  await listen('sidecar-status', (event) => {
    const { status, message } = event.payload;
    if (status === 'error') {
      showToast('Sidecar Error', message, 'error');
    }
  });

  await listen('auto-tokenized', (event) => {
    const { app_name, token_map } = event.payload;
    showToast('Auto-Tokenized', `Tokenized for ${app_name}`, 'success', 3000);
    state.stats.tokenized++;
    state.tokenMap = token_map || {};
    state.originalText = '';
    state.tokenizedText = '';
    state.entities = [];
    updateDashboard();
  });

  await listen('auto-detokenized', () => {
    showToast('Restored', 'Original PII restored from AI response', 'success', 3000);
    state.stats.detokenized++;
    updateDashboard();
  });

  await listen('history-updated', (event) => {
    state.history = event.payload;
    // If history tab is active, refresh it
    const histTab = document.querySelector('[data-tab="history"]');
    if (histTab && histTab.classList.contains('active')) {
      renderHistory();
    }
  });
}

// ── App Initialization ────────────────────────────────────────────────────────

async function init() {
  console.log('Initializing PII Shield…');

  initTabs();
  initSettingsListeners();

  elements.btnTokenize?.addEventListener('click', handleTokenize);
  elements.btnIgnore?.addEventListener('click', handleIgnore);
  elements.btnClearHistory?.addEventListener('click', handleClearHistory);
  elements.vaultClearAll?.addEventListener('click', handleClearVault);
  elements.btnViewVault?.addEventListener('click', () => {
    document.querySelector('[data-tab="vault"]').click();
  });

  await initTauriListeners();

  // Load config for Settings tab
  try {
    state.config = await invoke('get_config');
  } catch (err) {
    console.warn('Failed to load config:', err);
  }

  // Start clipboard monitoring
  try {
    await invoke('start_monitoring');
    console.log('Clipboard monitoring started');
    state.sidecarReady = true;
    elements.statusLabel.textContent = 'Monitoring';
  } catch (error) {
    console.error('Failed to start monitoring:', error);
    showToast('Error', 'Failed to start clipboard monitoring', 'error');
    elements.statusLabel.textContent = 'Error';
  }

  updateDashboard();
}

document.addEventListener('DOMContentLoaded', init);
