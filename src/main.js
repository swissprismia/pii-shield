import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';

// State
let state = {
  originalText: '',
  anonymizedText: '',
  entities: [],
  stats: {
    scanned: 0,
    detected: 0,
    anonymized: 0,
  },
};

// DOM Elements
const elements = {
  detectionSection: document.getElementById('detection-section'),
  emptySection: document.getElementById('empty-section'),
  detectionList: document.getElementById('detection-list'),
  originalText: document.getElementById('original-text'),
  anonymizedText: document.getElementById('anonymized-text'),
  piiCount: document.getElementById('pii-count'),
  btnAnonymize: document.getElementById('btn-anonymize'),
  btnIgnore: document.getElementById('btn-ignore'),
  statScanned: document.getElementById('stat-scanned'),
  statDetected: document.getElementById('stat-detected'),
  statAnonymized: document.getElementById('stat-anonymized'),
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
  elements.statAnonymized.textContent = state.stats.anonymized;

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
    elements.anonymizedText.textContent = state.anonymizedText;
  } else {
    // Show empty state
    elements.detectionSection.style.display = 'none';
    elements.emptySection.style.display = 'block';
  }
}

function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

// Handle anonymize button click
async function handleAnonymize() {
  try {
    console.log('✅ PASTED TO CLIPBOARD:', state.anonymizedText);

    // Copy anonymized text to clipboard
    await writeText(state.anonymizedText);

    // Update stats
    state.stats.anonymized++;

    // Reset detection state
    state.originalText = '';
    state.anonymizedText = '';
    state.entities = [];

    updateUI();

    showToast('Anonymized', 'Safe text copied to clipboard', 'success');

    // Notify backend that we've handled this clipboard content
    await invoke('mark_clipboard_handled');
  } catch (error) {
    console.error('Failed to copy anonymized text:', error);
    showToast('Error', 'Failed to copy to clipboard', 'error');
  }
}

// Handle ignore button click
async function handleIgnore() {
  // Reset detection state without copying
  state.originalText = '';
  state.anonymizedText = '';
  state.entities = [];

  updateUI();

  // Notify backend that we've handled this clipboard content
  try {
    await invoke('mark_clipboard_handled');
  } catch (error) {
    console.error('Failed to mark clipboard as handled:', error);
  }
}

// Initialize event listeners
function initEventListeners() {
  elements.btnAnonymize.addEventListener('click', handleAnonymize);
  elements.btnIgnore.addEventListener('click', handleIgnore);
}

// Listen for events from Rust backend
async function initTauriListeners() {
  // Listen for PII detection results
  await listen('pii-detected', (event) => {
    const { original_text, anonymized_text, entities } = event.payload;

    console.log('📋 COPIED:', original_text);
    console.log('⚠️ PII DETECTED:', entities.length, 'items');
    console.log('🔒 WILL PASTE:', anonymized_text);

    state.originalText = original_text;
    state.anonymizedText = anonymized_text;
    state.entities = entities;
    state.stats.scanned++;
    state.stats.detected += entities.length;

    updateUI();

    // Show toast notification
    showToast(
      `${entities.length} PII item${entities.length > 1 ? 's' : ''} detected`,
      'Click to view and anonymize',
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

  // Listen for auto-anonymization events
  await listen('auto-anonymized', (event) => {
    const { app_name } = event.payload;
    console.log('✅ Auto-anonymized for:', app_name);
    showToast('Auto-Anonymized', `Safe text ready to paste in ${app_name}`, 'success', 3000);

    // Update stats
    state.stats.anonymized++;

    // Clear detection state
    state.originalText = '';
    state.anonymizedText = '';
    state.entities = [];

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
