// Fixed JXA resource, run with osascript -l JavaScript. No source interpolation.
// argv: application PID, optional tty/pid getter names ("-" if absent), max
// windows, max total tabs, max total terminals. All queries address that PID.
// I 1 <windows> <terminals>; W <id> <selected tab> <tabs>;
// T <window> <id> <selected> <focused terminal> <terminals>;
// S <window> <tab> <id> <tty or -> <pid or ->; E <windows> <terminals>.
//
// Each read asks for one property of every element at one level in a single
// Apple event, answered from one state of the application; per-element reads
// cost one event each. The structure is read from the windows down, each level
// bounded before its children are asked for, then again from the terminals up
// and must match: a change to any level after its first read is seen by the
// second read of that level or of a parent, and the answer describes the
// application as it was at one instant.

function uniqueIds(ids) {
    return new Set(ids).size === ids.length;
}

function nativeId(id) {
    if (typeof id !== 'string' || !id.length || id.length > 256 || /[\x00-\x1f\x7f]/.test(id)) {
        throw Error('Invalid Ghostty native ID');
    }
    return id;
}

function changed() {
    return Error('Ghostty application changed during inventory');
}

function nested(level, parents) {
    return Array.isArray(level) && level.length === parents.length && level.every(Array.isArray);
}

function total(lists) {
    return lists.reduce((sum, list) => sum + list.length, 0);
}

function structure(app, maxWindows, maxTabs, maxTerminals) {
    const windows = app.windows.id();
    if (windows.length > maxWindows) throw Error('Ghostty inventory exceeds limit');
    const tabs = app.windows.tabs.id();
    if (!nested(tabs, windows)) throw changed();
    if (total(tabs) > maxTabs) throw Error('Ghostty inventory exceeds limit');
    const terminals = app.windows.tabs.terminals.id();
    if (!nested(terminals, windows) || !terminals.every((lists, w) => nested(lists, tabs[w]))) throw changed();
    if (total(terminals.flat()) > maxTerminals) throw Error('Ghostty inventory exceeds limit');
    return {
        windows,
        selectedTabs: app.windows.selectedTab.id(),
        tabs,
        selected: app.windows.tabs.selected(),
        focused: app.windows.tabs.focusedTerminal.id(),
        terminals,
        appTerminals: app.terminals.id(),
    };
}

// The same properties read from the terminals up, in the same key order.
function reread(app) {
    const appTerminals = app.terminals.id();
    const terminals = app.windows.tabs.terminals.id();
    const focused = app.windows.tabs.focusedTerminal.id();
    const selected = app.windows.tabs.selected();
    const tabs = app.windows.tabs.id();
    const selectedTabs = app.windows.selectedTab.id();
    const windows = app.windows.id();
    return { windows, selectedTabs, tabs, selected, focused, terminals, appTerminals };
}

// The same nesting as `terminals`: one list per window, one per tab.
function sameShape(values, terminals) {
    return Array.isArray(values) && values.length === terminals.length &&
        values.every((tabs, w) => Array.isArray(tabs) && tabs.length === terminals[w].length &&
            tabs.every((list, t) => Array.isArray(list) && list.length === terminals[w][t].length));
}

function validate(s, maxWindows, maxTabs, maxTerminals) {
    const count = s.windows.length;
    if (count > maxWindows) throw Error('Ghostty inventory exceeds limit');
    if ([s.selectedTabs, s.tabs, s.selected, s.focused, s.terminals]
        .some(level => !Array.isArray(level) || level.length !== count)) throw changed();
    s.windows.forEach(nativeId);
    s.selectedTabs.forEach(nativeId);
    const tabIds = [];
    const terminalIds = [];
    s.tabs.forEach((tabs, w) => {
        if (!Array.isArray(tabs)) throw changed();
        if (!tabs.length) throw Error('Ghostty window has no tabs');
        if (s.selected[w].length !== tabs.length || s.focused[w].length !== tabs.length ||
            s.terminals[w].length !== tabs.length) throw changed();
        tabs.forEach((tabId, t) => {
            tabIds.push(nativeId(tabId));
            if (typeof s.selected[w][t] !== 'boolean') throw Error('Invalid tab selection');
            nativeId(s.focused[w][t]);
            const terminals = s.terminals[w][t];
            if (!Array.isArray(terminals)) throw changed();
            if (!terminals.length) throw Error('Ghostty tab has no terminals');
            terminals.forEach(id => terminalIds.push(nativeId(id)));
        });
    });
    if (tabIds.length > maxTabs || terminalIds.length > maxTerminals ||
        s.appTerminals.length > maxTerminals) throw Error('Ghostty inventory exceeds limit');
    s.appTerminals.forEach(nativeId);
    if (!uniqueIds(s.windows) || !uniqueIds(tabIds) || !uniqueIds(terminalIds) ||
        !uniqueIds(s.appTerminals)) throw Error('Ghostty duplicate native IDs');
    if (terminalIds.length !== s.appTerminals.length ||
        !s.appTerminals.every(id => terminalIds.includes(id))) throw changed();
}

function optionalValues(app, property, terminals) {
    if (property === '-') return null;
    const values = app.windows.tabs.terminals[property]();
    if (!sameShape(values, terminals)) throw changed();
    return values;
}

function optionalValue(values, property, w, t, k) {
    if (values === null) return '-';
    const value = values[w][t][k];
    if (value === null || value === undefined) return '-';
    if (property === 'tty') {
        if (typeof value !== 'string') throw Error('Invalid tty property');
    } else if (!Number.isInteger(value) || value <= 0 || value > 2147483647) {
        throw Error('Invalid pid property');
    }
    return String(value);
}

function run(argv) {
    if (argv.length !== 6) throw Error('Ghostty inventory arguments');
    const appPid = Number(argv[0]);
    if (!Number.isInteger(appPid) || appPid <= 0 || appPid > 2147483647) throw Error('Invalid application PID');
    const ttyProperty = argv[1];
    const pidProperty = argv[2];
    [ttyProperty, pidProperty].forEach(property => {
        if (property !== '-' && !['tty', 'pid', 'processId'].includes(property)) throw Error('Invalid optional property');
    });
    const maxWindows = Number(argv[3]);
    const maxTabs = Number(argv[4]);
    const maxTerminals = Number(argv[5]);
    // The caller samples the PID's generation and probes its own executable's
    // dictionary. Neither a bundle ID nor /Applications chooses the recipient.
    const app = Application(appPid);
    if (!app.running()) throw Error('Ghostty is not running');
    const s = structure(app, maxWindows, maxTabs, maxTerminals);
    validate(s, maxWindows, maxTabs, maxTerminals);
    const ttys = optionalValues(app, ttyProperty, s.terminals);
    const pids = optionalValues(app, pidProperty, s.terminals);
    // Equal-count replacements, split moves, selection changes and
    // focused-terminal changes after the first read invalidate the snapshot.
    if (JSON.stringify(reread(app)) !== JSON.stringify(s)) throw changed();

    const rows = [['I', '1', s.windows.length, s.appTerminals.length]];
    s.windows.forEach((windowId, w) => {
        rows.push(['W', windowId, s.selectedTabs[w], s.tabs[w].length]);
        s.tabs[w].forEach((tabId, t) => {
            const terminals = s.terminals[w][t];
            rows.push(['T', windowId, tabId, s.selected[w][t], s.focused[w][t], terminals.length]);
            terminals.forEach((terminalId, k) => {
                rows.push(['S', windowId, tabId, terminalId,
                    optionalValue(ttys, ttyProperty, w, t, k), optionalValue(pids, pidProperty, w, t, k)]);
            });
        });
    });
    rows.push(['E', s.windows.length, s.appTerminals.length]);
    return rows.map(row => row.join('\t')).join('\n');
}
