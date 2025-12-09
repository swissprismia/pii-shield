import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';

// State
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
};

// DOM Elements
const elements = {
  detectionSection: document.getElementById('detection-section'),
  emptySection: document.getElementById('empty-section'),
  detectionList: document.getElementById('detection-list'),
  originalText: document.getElementById('original-text'),
  tokenizedText: document.getElementById('tokenized-text'),
  tokenMapSection: document.getElementById('token-map-section'),
  tokenMapList: document.getElementById('token-map-list'),
  piiCount: document.getElementById('pii-count'),
  btnTokenize: document.getElementById('btn-tokenize'),
  btnIgnore: document.getElementById('btn-ignore'),
  btnClearVault: document.getElementById('btn-clear-vault'),
  statScanned: document.getElementById('stat-scanned'),
  statDetected: document.getElementById('stat-detected'),
  statTokenized: document.getElementById('stat-tokenized'),
  statDetokenized: document.getElementById('stat-detokenized'),
  activeWindow: document.getElementById('active-window'),
  toastContainer: document.getElementById('toast-container'),
};

// Toast Notification System
function showToast(title, message, type = 'info', duration = 4000, onClick = null) {
  const toast = document.createElement('div');
  toast.className = `toast ${type}`;

  const icons = {
    success: '✓',
    warning: '⚠',
    error: '✕',
    info: 'ℹ',
  };

  toast.innerHTML = `
    <span class="toast-icon">${icons[type] || icons.info}</span>
    <div class="toast-content">
      <div class="toast-title">${title}</div>
      ${message ? `<div class="toast-message">${message}</div>` : ''}
    </div>
  `;

  if (onClick) {
    toast.style.cursor = 'pointer';
    toast.addEventListener('click', () => {
      onClick();
      removeToast(toast);
    });
  }

  elements.toastContainer.appendChild(toast);

  setTimeout(() => removeToast(toast), duration);

  return toast;
}

function removeToast(toast) {
  toast.style.animation = 'slideOut 0.3s ease forwards';
  setTimeout(() => toast.remove(), 300);
}

// Entity type display names and colors
const entityConfig = {
  PERSON: { label: 'Person', color: '#f59e0b' },
  EMAIL_ADDRESS: { label: 'Email', color: '#ef4444' },
  PHONE_NUMBER: { label: 'Phone', color: '#8b5cf6' },
  CREDIT_CARD: { label: 'Credit Card', color: '#ec4899' },
  US_SSN: { label: 'SSN', color: '#ef4444' },
  IP_ADDRESS: { label: 'IP Address', color: '#06b6d4' },
  URL: { label: 'URL', color: '#3b82f6' },
  LOCATION: { label: 'Location', color: '#22c55e' },
  DATE_TIME: { label: 'Date/Time', color: '#a855f7' },
  DOMAIN_NAME: { label: 'Domain', color: '#6366f1' },
  IBAN_CODE: { label: 'IBAN', color: '#f43f5e' },
  US_BANK_NUMBER: { label: 'Bank #', color: '#f43f5e' },
  US_PASSPORT: { label: 'Passport', color: '#dc2626' },
  NRP: { label: 'NRP', color: '#f97316' },
  MEDICAL_LICENSE: { label: 'Medical License', color: '#14b8a6' },
};

// Update UI based on state
function updateUI() {
  // Update stats
  elements.statScanned.textContent = state.stats.scanned;
  elements.statDetected.textContent = state.stats.detected;
  elements.statTokenized.textContent = state.stats.tokenized;
  elements.statDetokenized.textContent = state.stats.detokenized;

  if (state.entities.length > 0) {
    // Show detection section
    elements.detectionSection.style.display = 'block';
    elements.emptySection.style.display = 'none';

    // Update PII count badge
    elements.piiCount.textContent = state.entities.length;

    // Render entity tags
    elements.detectionList.innerHTML = state.entities
      .map((entity) => {
        const config = entityConfig[entity.entity_type] || { label: entity.entity_type, color: '#666' };
        const truncatedValue = entity.text.length > 30
          ? entity.text.substring(0, 30) + '...'
          : entity.text;
        return `
          <div class="pii-tag">
            <span class="type" style="color: ${config.color}">${config.label}</span>
            <span class="value">${escapeHtml(truncatedValue)}</span>
          </div>
        `;
      })
      .join('');

    // Update preview texts
    elements.originalText.textContent = state.originalText;
    elements.tokenizedText.textContent = state.tokenizedText;

    // Update token map display
    if (Object.keys(state.tokenMap).length > 0) {
      elements.tokenMapSection.style.display = 'block';
      elements.tokenMapList.innerHTML = Object.entries(state.tokenMap)
        .map(([token, value]) => {
          const truncatedValue = value.length > 20 ? value.substring(0, 20) + '...' : value;
          return `
            <div class="token-mapping">
              <span class="token-id">[${escapeHtml(token)}]</span>
              <span class="token-arrow">→</span>
              <span class="token-value">${escapeHtml(truncatedValue)}</span>
            </div>
          `;
        })
        .join('');
    } else {
      elements.tokenMapSection.style.display = 'none';
    }
  } else {
    // Show empty state
    elements.detectionSection.style.display = 'none';
    elements.emptySection.style.display = 'block';
    elements.tokenMapSection.style.display = 'none';
  }
}

function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

// Handle tokenize button click
async function handleTokenize() {
  try {
    console.log('Tokenizing and copying to clipboard:', state.tokenizedText);

    // Copy tokenized text to clipboard
    await writeText(state.tokenizedText);

    // Update stats
    state.stats.tokenized++;

    // Reset detection state
    state.originalText = '';
    state.tokenizedText = '';
    state.entities = [];
    // Keep tokenMap for de-tokenization later!

    updateUI();

    showToast('Tokenized', 'Tokenized text copied to clipboard', 'success');

    // Notify backend that we've handled this clipboard content
    await invoke('mark_clipboard_handled');
  } catch (error) {
    console.error('Failed to copy tokenized text:', error);
    showToast('Error', 'Failed to copy to clipboard', 'error');
  }
}

// Handle ignore button click
async function handleIgnore() {
  // Reset detection state without copying
  state.originalText = '';
  state.tokenizedText = '';
  state.entities = [];
  state.tokenMap = {};

  updateUI();

  // Notify backend that we've handled this clipboard content
  try {
    await invoke('mark_clipboard_handled');
  } catch (error) {
    console.error('Failed to mark clipboard as handled:', error);
  }
}

// Handle clear vault button click
async function handleClearVault() {
  try {
    await invoke('clear_token_vault');
    state.tokenMap = {};
    showToast('Vault Cleared', 'Token mappings have been cleared', 'info');
  } catch (error) {
    console.error('Failed to clear token vault:', error);
    showToast('Error', 'Failed to clear token vault', 'error');
  }
}

// Initialize event listeners
function initEventListeners() {
  elements.btnTokenize.addEventListener('click', handleTokenize);
  elements.btnIgnore.addEventListener('click', handleIgnore);
  if (elements.btnClearVault) {
    elements.btnClearVault.addEventListener('click', handleClearVault);
  }
}

// Listen for events from Rust backend
async function initTauriListeners() {
  // Listen for PII detection results (now includes tokenization)
  await listen('pii-detected', (event) => {
    const { original_text, tokenized_text, token_map, entities } = event.payload;

    console.log('Copied:', original_text);
    console.log('PII Detected:', entities.length, 'items');
    console.log('Tokenized:', tokenized_text);
    console.log('Token map:', token_map);

    state.originalText = original_text;
    state.tokenizedText = tokenized_text;
    state.tokenMap = token_map || {};
    state.entities = entities;
    state.stats.scanned++;
    state.stats.detected += entities.length;

    updateUI();

    // Show toast notification
    showToast(
      `${entities.length} PII item${entities.length > 1 ? 's' : ''} detected`,
      'Click to view and tokenize',
      'warning',
      5000,
      () => {
        // Focus the detection section (already visible)
        elements.detectionSection.scrollIntoView({ behavior: 'smooth' });
      }
    );
  });

  // Listen for active window changes
  await listen('active-window-changed', (event) => {
    const { title, app_name } = event.payload;
    elements.activeWindow.textContent = app_name || title || '—';
  });

  // Listen for clipboard scan events (no PII found)
  await listen('clipboard-scanned', (event) => {
    state.stats.scanned++;
    updateUI();
  });

  // Listen for sidecar status
  await listen('sidecar-status', (event) => {
    const { status, message } = event.payload;
    if (status === 'error') {
      showToast('Sidecar Error', message, 'error');
    }
  });

  // Listen for auto-tokenization events (when pasting in browser)
  await listen('auto-tokenized', (event) => {
    const { app_name, tokenized_text, token_map } = event.payload;
    console.log('Auto-tokenized for:', app_name);
    console.log('Tokenized text:', tokenized_text);
    showToast('Auto-Tokenized', `Tokenized text ready to paste in ${app_name}`, 'success', 3000);

    // Update stats
    state.stats.tokenized++;

    // Store token map for later de-tokenization
    state.tokenMap = token_map || {};

    // Clear detection state
    state.originalText = '';
    state.tokenizedText = '';
    state.entities = [];

    updateUI();
  });

  // Listen for auto-detokenization events (when copying AI response with tokens)
  await listen('auto-detokenized', (event) => {
    const { original_text, detokenized_text, token_map } = event.payload;
    console.log('Auto-detokenized!');
    console.log('Original (with tokens):', original_text);
    console.log('Detokenized:', detokenized_text);
    showToast('Auto-Detokenized', 'Original PII restored from AI response', 'success', 3000);

    // Update stats
    state.stats.detokenized++;

    updateUI();
  });
}

// Initialize the application
async function init() {
  console.log('Initializing PII Shield...');

  initEventListeners();
  await initTauriListeners();

  // Start clipboard monitoring
  try {
    await invoke('start_monitoring');
    console.log('Clipboard monitoring started');
  } catch (error) {
    console.error('Failed to start monitoring:', error);
    showToast('Error', 'Failed to start clipboard monitoring', 'error');
  }

  updateUI();
}

// Start the app when DOM is ready
document.addEventListener('DOMContentLoaded', init);
