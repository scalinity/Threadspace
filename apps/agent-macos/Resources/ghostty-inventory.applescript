// Fixed JXA resource, run with osascript -l JavaScript. No source interpolation.
// argv: application PID, optional tty/pid getter names ("-" if absent), max
// windows, max total tabs, max total terminals. All queries address that PID.
// I 1 <windows> <terminals>; W <id> <selected tab> <tabs>;
// T <window> <id> <selected> <focused terminal> <terminals>;
// S <window> <tab> <id> <tty or -> <pid or ->; E <windows> <terminals>.

function uniqueIds(ids) {
    return new Set(ids).size === ids.length;
}

function sameIds(a, b) {
    return a.length === b.length && a.every((id, index) => id === b[index]);
}

function nativeId(object) {
    const id = object.id();
    if (typeof id !== 'string' || !id.length || id.length > 256 || /[\x00-\x1f\x7f]/.test(id)) {
        throw Error('Invalid Ghostty native ID');
    }
    return id;
}

function idsOf(objects, maximum) {
    if (objects.length > maximum) throw Error('Ghostty inventory exceeds limit');
    const ids = objects.map(nativeId);
    if (!uniqueIds(ids)) throw Error('Ghostty duplicate native IDs');
    return ids;
}

function optionalProperty(terminal, property) {
    if (property === '-') return '-';
    if (!['tty', 'pid', 'processId'].includes(property)) throw Error('Invalid optional property');
    const value = terminal[property]();
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
    const maxWindows = Number(argv[3]);
    const maxTabs = Number(argv[4]);
    const maxTerminals = Number(argv[5]);
    // The caller samples the PID's generation and probes its own executable's
    // dictionary. Neither a bundle ID nor /Applications chooses the recipient.
    const app = Application(appPid);
    if (!app.running()) throw Error('Ghostty is not running');
    const windowIds = idsOf(app.windows(), maxWindows);
    const appTerminalIds = idsOf(app.terminals(), maxTerminals);
    const rows = [['I', '1', windowIds.length, appTerminalIds.length]];
    const seenTabs = [];
    const seenTerminals = [];
    const windowSnapshots = [];
    const tabSnapshots = [];

    windowIds.forEach(windowId => {
        const window = app.windows.byId(windowId);
        const selectedTabId = nativeId(window.selectedTab());
        const tabIds = idsOf(window.tabs(), maxTabs - seenTabs.length);
        if (!tabIds.length) throw Error('Ghostty window has no tabs');
        windowSnapshots.push({ id: windowId, tabIds, selectedTabId });
        rows.push(['W', windowId, selectedTabId, tabIds.length]);
        tabIds.forEach(tabId => {
            seenTabs.push(tabId);
            const tab = window.tabs.byId(tabId);
            const selected = tab.selected();
            if (typeof selected !== 'boolean') throw Error('Invalid tab selection');
            const focusedId = nativeId(tab.focusedTerminal());
            const terminalIds = idsOf(tab.terminals(), maxTerminals - seenTerminals.length);
            if (!terminalIds.length) throw Error('Ghostty tab has no terminals');
            tabSnapshots.push({ windowId, id: tabId, terminalIds, focusedId, selected });
            rows.push(['T', windowId, tabId, selected, focusedId, terminalIds.length]);
            terminalIds.forEach(terminalId => {
                seenTerminals.push(terminalId);
                const terminal = tab.terminals.byId(terminalId);
                rows.push(['S', windowId, tabId, terminalId,
                    optionalProperty(terminal, ttyProperty), optionalProperty(terminal, pidProperty)]);
            });
        });
    });

    // Revisit earlier windows too: equal-count replacements, split moves,
    // selection changes and focused-terminal changes invalidate the snapshot.
    windowSnapshots.forEach(snapshot => {
        const window = app.windows.byId(snapshot.id);
        if (!sameIds(idsOf(window.tabs(), maxTabs), snapshot.tabIds) ||
            nativeId(window.selectedTab()) !== snapshot.selectedTabId) {
            throw Error('Ghostty window changed during inventory');
        }
    });
    tabSnapshots.forEach(snapshot => {
        const tab = app.windows.byId(snapshot.windowId).tabs.byId(snapshot.id);
        if (!sameIds(idsOf(tab.terminals(), maxTerminals), snapshot.terminalIds) ||
            nativeId(tab.focusedTerminal()) !== snapshot.focusedId || tab.selected() !== snapshot.selected) {
            throw Error('Ghostty tab changed during inventory');
        }
    });
    if (!sameIds(idsOf(app.windows(), maxWindows), windowIds) ||
        !sameIds(idsOf(app.terminals(), maxTerminals), appTerminalIds) ||
        !uniqueIds(seenTabs) || !uniqueIds(seenTerminals) ||
        seenTerminals.length !== appTerminalIds.length ||
        !appTerminalIds.every(id => seenTerminals.includes(id))) {
        throw Error('Ghostty application changed during inventory');
    }
    rows.push(['E', windowIds.length, appTerminalIds.length]);
    return rows.map(row => row.join('\t')).join('\n');
}
