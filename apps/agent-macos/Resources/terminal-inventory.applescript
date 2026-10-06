-- Read-only Terminal inventory (SPEC §13.3). Fixed bundled script run by the
-- companion's bounded osascript worker. It never selects, focuses, activates,
-- types or reads terminal contents. The companion only runs it while
-- Terminal is already running.
--
-- Output, one record per line, fields separated by TAB:
--   W <window id> <window index> <miniaturized> <tty count> <selected count>
--   T <window id> <tab index> <selected> <tty>
-- A window's T lines follow its W line; the counts let the reader reject an
-- enumeration that raced with a tab being added or closed.
on run argv
	-- Bound outside the tell block: inside it, `tab` names Terminal's tab class.
	set separator to character id 9
	set rows to {}
	tell application "Terminal"
		set windowIds to id of every window
		repeat with windowRef in windowIds
			set windowId to contents of windowRef
			set theWindow to window id windowId
			set windowIndex to index of theWindow
			set isMini to miniaturized of theWindow
			set tabTtys to tty of every tab of theWindow
			set tabSelected to selected of every tab of theWindow
			set end of rows to "W" & separator & windowId & separator & windowIndex & separator & isMini & separator & (count of tabTtys) & separator & (count of tabSelected)
			repeat with i from 1 to count of tabTtys
				set end of rows to "T" & separator & windowId & separator & i & separator & (item i of tabSelected) & separator & (item i of tabTtys)
			end repeat
		end repeat
	end tell
	set AppleScript's text item delimiters to linefeed
	set output to rows as text
	set AppleScript's text item delimiters to ""
	return output
end run
