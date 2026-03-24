// Tauri IPC — use the global __TAURI__ injected by Tauri
const { invoke } = window.__TAURI__.core;

// State
let treeCursor = 0;
let flatTree = [];
let focusedPane = 'tree'; // 'tree' or 'viewer'
let showTree = true;
let pendingPrefix = null;
let currentFile = null;
let fontSize = 16;

// Init
async function init() {
  const config = await invoke('get_config');
  fontSize = config.font.size;
  applyFontSize();
  if (config.ui.theme) {
    document.body.className = 'theme-' + config.ui.theme;
  }

  const initialPath = await invoke('get_initial_path');
  await loadTree();

  if (initialPath) {
    await openFile(initialPath);
  }
}

// Tree
async function loadTree() {
  const response = await invoke('get_tree');
  flatTree = response.entries;
  renderTree();
}

function renderTree() {
  const pane = document.getElementById('tree-pane');
  pane.innerHTML = '';

  flatTree.forEach((entry, idx) => {
    const btn = document.createElement('button');
    btn.className = 'tree-entry';
    if (idx === treeCursor && focusedPane === 'tree') btn.classList.add('selected');
    if (entry.is_current) btn.classList.add('current');
    if (entry.is_dir) btn.classList.add('directory');

    const indent = '\u00A0\u00A0'.repeat(entry.depth);
    const icon = entry.is_dir ? (entry.is_expanded ? '▾ ' : '▸ ') : '\u00A0\u00A0';
    btn.textContent = indent + icon + entry.name;

    btn.addEventListener('click', () => {
      treeCursor = idx;
      if (entry.is_dir) {
        toggleDir(entry.path);
      } else {
        openFile(entry.path);
      }
    });

    pane.appendChild(btn);
  });

  scrollTreeCursorIntoView();
}

function scrollTreeCursorIntoView() {
  const pane = document.getElementById('tree-pane');
  const entries = pane.querySelectorAll('.tree-entry');
  if (entries[treeCursor]) {
    entries[treeCursor].scrollIntoView({ block: 'nearest' });
  }
}

async function toggleDir(path) {
  await invoke('toggle_dir', { path });
  await loadTree();
}

async function saveCurrentPosition() {
  if (currentFile) {
    const offset = document.getElementById('viewer-content').scrollTop;
    await invoke('save_position', { path: currentFile, offset });
  }
}

async function openFile(path) {
  // Save position of previous file
  await saveCurrentPosition();

  const response = await invoke('open_file', { path });
  currentFile = path;
  document.getElementById('viewer-header').textContent = response.filename;
  document.getElementById('viewer-content').innerHTML = response.html;

  // Restore saved scroll position
  const saved = await invoke('load_position', { path });
  if (saved != null) {
    document.getElementById('viewer-content').scrollTop = saved;
  } else {
    document.getElementById('viewer-content').scrollTop = 0;
  }

  focusedPane = 'viewer';
  await loadTree();
}

function closeFile() {
  saveCurrentPosition();
  currentFile = null;
  document.getElementById('viewer-header').textContent = '';
  document.getElementById('viewer-content').innerHTML =
    '<p class="placeholder">Open a file from the tree to start reading.</p>';
  loadTree();
}

async function reloadFile() {
  const result = await invoke('reload_file');
  if (result) {
    const scrollPos = document.getElementById('viewer-content').scrollTop;
    document.getElementById('viewer-header').textContent = result.filename;
    document.getElementById('viewer-content').innerHTML = result.html;
    document.getElementById('viewer-content').scrollTop = scrollPos;
    showToast('Reloaded');
  }
}

async function refreshTree() {
  const response = await invoke('refresh_tree');
  flatTree = response.entries;
  renderTree();
  showToast('Tree refreshed');
}

// Dir input
function showDirInput() {
  const bar = document.getElementById('dir-input-bar');
  const input = document.getElementById('dir-input');
  bar.classList.remove('hidden');
  input.value = '';
  input.focus();
}

function hideDirInput() {
  document.getElementById('dir-input-bar').classList.add('hidden');
}

document.getElementById('dir-input').addEventListener('keydown', async (e) => {
  if (e.key === 'Enter') {
    const path = document.getElementById('dir-input').value;
    if (path) {
      await invoke('change_directory', { path });
      currentFile = null;
      document.getElementById('viewer-header').textContent = '';
      document.getElementById('viewer-content').innerHTML =
        '<p class="placeholder">Open a file from the tree to start reading.</p>';
      await loadTree();
      treeCursor = 0;
    }
    hideDirInput();
    e.preventDefault();
  } else if (e.key === 'Escape') {
    hideDirInput();
    e.preventDefault();
  }
  e.stopPropagation(); // Don't let keybindings fire while typing
});

// Help
function toggleHelp() {
  document.getElementById('help-overlay').classList.toggle('hidden');
}

// Font size — goes through Rust to persist in config
function applyFontSize() {
  document.documentElement.style.setProperty('--font-size', fontSize + 'px');
  showToast(`Font size: ${fontSize}px`);
}

function showToast(msg) {
  let el = document.getElementById('toast');
  if (!el) {
    el = document.createElement('div');
    el.id = 'toast';
    el.style.cssText = 'position:fixed;bottom:32px;right:32px;background:var(--bg-header);color:var(--fg);padding:6px 16px;border-radius:4px;border:1px solid var(--border);font-size:14px;opacity:0;transition:opacity 0.2s;z-index:200;pointer-events:none';
    document.body.appendChild(el);
  }
  el.textContent = msg;
  el.style.opacity = '1';
  clearTimeout(el._timer);
  el._timer = setTimeout(() => { el.style.opacity = '0'; }, 1500);
}

async function adjustFontSize(delta) {
  fontSize = await invoke('adjust_font_size', { delta });
  applyFontSize();
}

async function resetFontSize() {
  fontSize = await invoke('adjust_font_size', { delta: 0.0 });
  applyFontSize();
}

// Toggle tree pane
function toggleTreePane() {
  showTree = !showTree;
  document.getElementById('tree-pane').classList.toggle('hidden', !showTree);
}

// Cycle theme
async function cycleTheme() {
  const newTheme = await invoke('cycle_theme');
  document.body.className = 'theme-' + newTheme;
}

// Scrolling
function scrollViewer(lines) {
  const el = document.getElementById('viewer-content');
  el.scrollBy(0, lines * 24);
}

function pageViewer(pages) {
  const el = document.getElementById('viewer-content');
  el.scrollBy(0, pages * el.clientHeight * 0.9);
}

// Keybindings
document.addEventListener('keydown', (e) => {
  // Don't intercept when dir input is focused
  if (document.activeElement === document.getElementById('dir-input')) return;

  const helpVisible = !document.getElementById('help-overlay').classList.contains('hidden');

  // Escape: dismiss overlays, cancel chords, or start ESC-as-Meta prefix
  if (e.key === 'Escape') {
    if (helpVisible) { toggleHelp(); e.preventDefault(); return; }
    if (pendingPrefix) { pendingPrefix = null; e.preventDefault(); return; }
    // ESC acts as Meta prefix (emacs: ESC x = M-x)
    pendingPrefix = 'escape';
    e.preventDefault();
    return;
  }

  // C-h toggles help even when help is visible
  if (e.ctrlKey && e.key === 'h') { toggleHelp(); e.preventDefault(); return; }

  // Block other keys when help is visible
  if (helpVisible) return;

  // Chord handling: C-x prefix
  if (e.ctrlKey && !e.altKey && e.key === 'x') {
    pendingPrefix = 'x';
    e.preventDefault();
    return;
  }

  if (pendingPrefix === 'escape') {
    // ESC prefix: treat next key as Alt+key
    pendingPrefix = null;
    // Synthesize an Alt key event by dispatching to the Alt handler
    const key = e.key;
    e.preventDefault();
    switch (key) {
      case 'v': pageViewer(-1); return;
      case 't': cycleTheme(); return;
      case '<': case ',':
        document.getElementById('viewer-content').scrollTop = 0; return;
      case '>': case '.':
        { const el = document.getElementById('viewer-content'); el.scrollTop = el.scrollHeight; } return;
    }
    return;
  }

  if (pendingPrefix === 'x') {
    pendingPrefix = null;
    if (e.ctrlKey && e.key === 'f') { showDirInput(); e.preventDefault(); return; }
    if (e.ctrlKey && e.key === 'c') { saveCurrentPosition().then(() => invoke('quit')); e.preventDefault(); return; }
    e.preventDefault();
    return;
  }

  // Single keybindings
  if (e.ctrlKey && !e.altKey) {
    switch (e.key) {
      case 'n':
        if (focusedPane === 'viewer') scrollViewer(1);
        else { treeCursor = Math.min(treeCursor + 1, flatTree.length - 1); renderTree(); }
        e.preventDefault(); return;
      case 'p':
        if (focusedPane === 'viewer') scrollViewer(-1);
        else { treeCursor = Math.max(treeCursor - 1, 0); renderTree(); }
        e.preventDefault(); return;
      case 'v':
        pageViewer(1);
        e.preventDefault(); return;
      case 'f':
        if (focusedPane === 'tree') {
          const entry = flatTree[treeCursor];
          if (entry && entry.is_dir && !entry.is_expanded) toggleDir(entry.path);
        } else {
          scrollViewer(1);
        }
        e.preventDefault(); return;
      case 'b':
        if (focusedPane === 'tree') {
          const entry = flatTree[treeCursor];
          if (entry && entry.is_dir && entry.is_expanded) toggleDir(entry.path);
        } else {
          scrollViewer(-1);
        }
        e.preventDefault(); return;
      case 'w':
        closeFile();
        e.preventDefault(); return;
      case 'r':
        if (focusedPane === 'tree') refreshTree();
        else reloadFile();
        e.preventDefault(); return;
      case 's':
        // Trigger browser-native find-in-page
        // We can't programmatically open Ctrl+F in WebKit, so we let it through
        return; // Don't preventDefault — let the browser handle C-s as find
      case 't':
        toggleTreePane();
        e.preventDefault(); return;
      case '=':
      case '+':
        adjustFontSize(2);
        e.preventDefault(); return;
      case '-':
        adjustFontSize(-2);
        e.preventDefault(); return;
      case '0':
        resetFontSize();
        e.preventDefault(); return;
    }
  }

  if (e.altKey && !e.ctrlKey) {
    switch (e.key) {
      case 'v':
        pageViewer(-1);
        e.preventDefault(); return;
      case 't':
        cycleTheme();
        e.preventDefault(); return;
      case '<':
      case ',':
        document.getElementById('viewer-content').scrollTop = 0;
        e.preventDefault(); return;
      case '>':
      case '.':
        const el = document.getElementById('viewer-content');
        el.scrollTop = el.scrollHeight;
        e.preventDefault(); return;
    }
  }

  // Non-modifier keys
  if (!e.ctrlKey && !e.altKey && !e.metaKey) {
    switch (e.key) {
      case 'Tab':
        focusedPane = focusedPane === 'tree' ? 'viewer' : 'tree';
        renderTree();
        e.preventDefault(); return;
      case 'Enter':
        if (focusedPane === 'tree' && flatTree[treeCursor]) {
          const entry = flatTree[treeCursor];
          if (entry.is_dir) toggleDir(entry.path);
          else openFile(entry.path);
        }
        e.preventDefault(); return;
      case 'q':
        saveCurrentPosition().then(() => invoke('quit'));
        e.preventDefault(); return;
    }
  }
});

// Start
init();
