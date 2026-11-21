#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
app=dev.punkhomov.tracking
runner="$app.test/androidx.test.runner.AndroidJUnitRunner"

adb install -r app/build/outputs/apk/debug/app-debug.apk
adb install -r app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb shell pm clear "$app"

run_test() {
    local output
    output=$(adb shell am instrument -w -e class "$app.TimerSmokeTest#$1" "$runner")
    printf '%s\n' "$output"
    grep -Fq 'OK (1 test)' <<< "$output"
}

run_test startSession
adb shell am force-stop "$app"
run_test restoreAndFinishSession
run_test createAndArchiveCategory
