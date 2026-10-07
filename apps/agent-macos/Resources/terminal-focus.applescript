-- Exact Terminal tab focus (SPEC §13.3). Fixed bundled script run by the
-- companion's bounded osascript worker; the TTY path arrives as a data
-- argument and is never interpolated into source; the second argument is
-- what remains of the route's budget, in milliseconds. It selects one live tab,
-- unminimizes and raises its current window and activates Terminal. It never
-- types, sends a newline, runs a command or reads terminal contents.
--
-- The caller has already proven, from fresh enumeration and stat(st_rdev),
-- that exactly one tab carries the provider's controlling device. This script
-- re-checks uniqueness and the tab's TTY immediately before selecting it and
-- refuses instead of guessing.
--
-- Output, fields separated by TAB:
--   GONE                      no tab has the TTY
--   AMBIGUOUS <count>         more than one tab has the TTY
--   CHANGED                   the located tab's TTY changed before selection
--   FOCUSED <window id> <tab index> <front window id> <front selected tty>
--           <target window frontmost> <target tab selected>
on run argv
	set separator to character id 9
	if (count of argv) is not 2 then return "USAGE"
	set targetTty to item 1 of argv
	set budgetMs to (item 2 of argv) as integer
	tell application "Terminal"
		set matchCount to 0
		set matchWindowId to 0
		set matchTabIndex to 0
		set windowIds to id of every window
		repeat with windowRef in windowIds
			set windowId to contents of windowRef
			set tabTtys to tty of every tab of window id windowId
			repeat with i from 1 to count of tabTtys
				if (item i of tabTtys) is targetTty then
					set matchCount to matchCount + 1
					set matchWindowId to windowId
					set matchTabIndex to i
				end if
			end repeat
		end repeat
		if matchCount is 0 then return "GONE"
		if matchCount is greater than 1 then return "AMBIGUOUS" & separator & matchCount
		set targetWindow to window id matchWindowId
		set targetTab to tab matchTabIndex of targetWindow
		if (tty of targetTab) is not targetTty then return "CHANGED"
		if miniaturized of targetWindow then set miniaturized of targetWindow to false
		set selected of targetTab to true
		set index of targetWindow to 1
		activate
		-- Showing a window that lives in another Space, such as its own
		-- fullscreen Space, completes asynchronously: read back once the
		-- target leads Terminal's window order. The polling never sleeps
		-- longer than the route's remaining budget, and the companion stops
		-- this script at the route deadline, which also caps the time the
		-- queries themselves take. The readback below is unchanged and must
		-- still match.
		repeat (budgetMs div 50) times
			if (id of front window) is matchWindowId then exit repeat
			delay 0.05
		end repeat
		set frontWindow to front window
		return "FOCUSED" & separator & matchWindowId & separator & matchTabIndex & separator & (id of frontWindow) & separator & (tty of selected tab of frontWindow) & separator & (frontmost of targetWindow) & separator & (selected of targetTab)
	end tell
end run
