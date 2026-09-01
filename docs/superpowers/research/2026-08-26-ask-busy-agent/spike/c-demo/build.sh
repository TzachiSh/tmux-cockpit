#!/bin/sh
exec gcc -O2 -Wall -Wextra $(pkg-config --cflags ncursesw) -o "$(dirname "$0")/cockpit-fleet-c" "$(dirname "$0")/fleet.c" $(pkg-config --libs ncursesw)
