# Superseded: CF raised attention after Stop (harness ordering)

Stop leaves the store prepared for maintenance and the cold-started writer inherits it, so raising the scenario's attention items on that writer was refused (MaintenanceGated, as designed). The runner now raises them on the login item's companion before Stop. No scenario completed.
