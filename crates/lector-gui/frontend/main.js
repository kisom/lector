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

// Open path bar (C-x C-f) with tab completion
function showOpenBar() {
  const bar = document.getElementById('open-bar');
  const input = document.getElementById('open-input');
  bar.classList.remove('hidden');
  input.value = '';
  input.focus();
}

function hideOpenBar() {
  document.getElementById('open-bar').classList.add('hidden');
}

async function handleOpenPath(path) {
  if (!path) return;
  try {
    const result = await invoke('open_path', { path });
    if (result) {
      // File was opened
      currentFile = result.path || path;
      document.getElementById('viewer-header').textContent = result.filename;
      document.getElementById('viewer-content').innerHTML = result.html;
      document.getElementById('viewer-content').scrollTop = 0;
      focusedPane = 'viewer';
    } else {
      // Directory was changed
      currentFile = null;
      document.getElementById('viewer-header').textContent = '';
      document.getElementById('viewer-content').innerHTML =
        '<p class="placeholder">Open a file from the tree to start reading.</p>';
    }
    await loadTree();
    treeCursor = 0;
  } catch (err) {
    showToast('Error: ' + err);
  }
}

document.getElementById('open-input').addEventListener('keydown', async (e) => {
  if (e.key === 'Enter') {
    const path = document.getElementById('open-input').value;
    hideOpenBar();
    await handleOpenPath(path);
    e.preventDefault();
  } else if (e.key === 'Escape' || (e.ctrlKey && e.key === 'g')) {
    hideOpenBar();
    e.preventDefault();
  } else if (e.key === 'Tab') {
    // Tab completion
    e.preventDefault();
    const input = document.getElementById('open-input');
    const completions = await invoke('complete_path', { input: input.value });
    if (completions.length === 1) {
      input.value = completions[0];
    } else if (completions.length > 1) {
      // Find common prefix
      let common = completions[0];
      for (const c of completions) {
        while (!c.startsWith(common)) {
          common = common.slice(0, -1);
        }
      }
      if (common.length > input.value.length) {
        input.value = common;
      } else {
        showToast(completions.map(c => c.split('/').pop() || c).join('  '));
      }
    }
  }
  e.stopPropagation();
});

// Visual file browser (C-o)
let browserEntries = [];
let browserCursor = 0;
let browserDir = '';

async function showFileBrowser() {
  // Start from current file's directory or tree root
  const startDir = currentFile
    ? currentFile.substring(0, currentFile.lastIndexOf('/'))
    : flatTree[0]?.path || '/';
  await loadBrowserDir(startDir);
  document.getElementById('file-browser').classList.remove('hidden');
  document.getElementById('browser-filter-input').value = '';
  document.getElementById('browser-filter-input').focus();
}

function hideFileBrowser() {
  document.getElementById('file-browser').classList.add('hidden');
}

async function loadBrowserDir(dir) {
  browserDir = dir;
  browserEntries = await invoke('browse_directory', { path: dir });
  browserCursor = 0;
  document.getElementById('file-browser-header').textContent = dir;
  renderBrowser('');
}

function renderBrowser(filter) {
  const list = document.getElementById('file-browser-list');
  list.innerHTML = '';

  const filtered = filter
    ? browserEntries.filter(e => e.name.toLowerCase().includes(filter.toLowerCase()))
    : browserEntries;

  if (browserCursor >= filtered.length) browserCursor = Math.max(0, filtered.length - 1);

  filtered.forEach((entry, idx) => {
    const btn = document.createElement('button');
    btn.className = 'browser-entry';
    if (idx === browserCursor) btn.classList.add('selected');
    if (entry.is_dir) btn.classList.add('directory');

    const icon = entry.is_dir ? '📁 ' : '  ';
    btn.textContent = icon + entry.name;

    btn.addEventListener('click', () => {
      browserCursor = idx;
      selectBrowserEntry(filtered);
    });

    list.appendChild(btn);
  });

  // Scroll selected into view
  const selected = list.querySelector('.selected');
  if (selected) selected.scrollIntoView({ block: 'nearest' });
}

async function selectBrowserEntry(filtered) {
  const entry = filtered[browserCursor];
  if (!entry) {
    // No entry selected (empty list or no matches) — open the current directory
    hideFileBrowser();
    await handleOpenPath(browserDir);
    return;
  }

  if (entry.is_dir) {
    await loadBrowserDir(entry.path);
    document.getElementById('browser-filter-input').value = '';
  } else {
    hideFileBrowser();
    await handleOpenPath(entry.path);
  }
}

document.getElementById('browser-filter-input').addEventListener('keydown', async (e) => {
  const filter = document.getElementById('browser-filter-input').value;
  const filtered = filter
    ? browserEntries.filter(en => en.name.toLowerCase().includes(filter.toLowerCase()))
    : browserEntries;

  if (e.key === 'Enter') {
    await selectBrowserEntry(filtered);
    e.preventDefault();
  } else if (e.key === 'Escape' || (e.ctrlKey && e.key === 'g')) {
    hideFileBrowser();
    e.preventDefault();
  } else if ((e.ctrlKey && e.key === 'n') || e.key === 'ArrowDown') {
    browserCursor = Math.min(browserCursor + 1, filtered.length - 1);
    renderBrowser(filter);
    e.preventDefault();
  } else if ((e.ctrlKey && e.key === 'p') || e.key === 'ArrowUp') {
    browserCursor = Math.max(browserCursor - 1, 0);
    renderBrowser(filter);
    e.preventDefault();
  }
  e.stopPropagation();
});

document.getElementById('browser-filter-input').addEventListener('input', (e) => {
  browserCursor = 0;
  renderBrowser(e.target.value);
});

// Search
function showSearch() {
  const bar = document.getElementById('search-bar');
  const input = document.getElementById('search-input');
  bar.classList.remove('hidden');
  input.value = '';
  document.getElementById('search-count').textContent = '';
  input.focus();
}

function hideSearch() {
  document.getElementById('search-bar').classList.add('hidden');
  // Clear selection/highlights
  window.getSelection().removeAllRanges();
}

function performSearch(query, forward) {
  if (!query) return;
  // window.find(string, caseSensitive, backwards, wrapAround)
  const found = window.find(query, false, !forward, true);
  document.getElementById('search-count').textContent = found ? '' : 'not found';
}

document.getElementById('search-input').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') {
    performSearch(document.getElementById('search-input').value, !e.shiftKey);
    e.preventDefault();
  } else if (e.key === 'Escape' || (e.ctrlKey && e.key === 'g')) {
    hideSearch();
    e.preventDefault();
  } else if (e.ctrlKey && e.key === 's') {
    // C-s while in search = find next
    performSearch(document.getElementById('search-input').value, true);
    e.preventDefault();
  } else if (e.ctrlKey && e.key === 'r') {
    // C-r while in search = find previous
    performSearch(document.getElementById('search-input').value, false);
    e.preventDefault();
  }
  e.stopPropagation();
});

document.getElementById('search-input').addEventListener('input', (e) => {
  // Incremental search: search as you type
  performSearch(e.target.value, true);
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
  // Don't intercept when an input field is focused
  const active = document.activeElement;
  if (active && (active.tagName === 'INPUT' || active.tagName === 'TEXTAREA')) return;

  const helpVisible = !document.getElementById('help-overlay').classList.contains('hidden');

  // C-g: emacs "cancel" — dismiss overlays, cancel chords (never starts meta prefix)
  if (e.ctrlKey && e.key === 'g') {
    if (helpVisible) { toggleHelp(); e.preventDefault(); return; }
    pendingPrefix = null;
    e.preventDefault();
    return;
  }

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
    if (e.ctrlKey && e.key === 'f') { showOpenBar(); e.preventDefault(); return; }
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
        showSearch();
        e.preventDefault(); return;
      case 'o':
        showFileBrowser();
        e.preventDefault(); return;
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
