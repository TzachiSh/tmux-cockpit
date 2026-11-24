/*
 * tests/cutclock.c: tmux's own CLOCK_MONOTONIC, one second later on cue, so that tmux's
 * 100 ms format time limit (tmux 3.7+, FORMAT_TIME_LIMIT in format.c) cuts a format at a
 * chosen call (#736). A block of tests/run.sh builds it and LD_PRELOADs it into one private
 * server's start. In a process not named tmux it does nothing. In tmux it takes itself out of
 * the environment first, so no pane or job of that server loads it.
 *
 * CUTCLOCK_ARM names a file. At each CLOCK_MONOTONIC call, if the file exists, its number J
 * is read, the file is removed and the count restarts, this call being 1. Call J and every
 * later one read one second more, and the count stops until the next file. J = 0 only stops
 * it. CUTCLOCK_LOG, when set, gets a line per counted call, so a block can count the calls a
 * command makes and walk J over every one of them.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static int (*real_cg)(clockid_t, struct timespec *);
static char armp[4096], logp[4096];
static int on;
static long target, n;
static time_t jump;

__attribute__((constructor)) static void
cutclock_init(void)
{
	const char *a = getenv("CUTCLOCK_ARM"), *l = getenv("CUTCLOCK_LOG");

	if (strcmp(program_invocation_short_name, "tmux") != 0)
		return;
	if (a != NULL && *a == '/' && strlen(a) < sizeof armp) {
		strcpy(armp, a);
		on = 1;
	}
	if (l != NULL && *l == '/' && strlen(l) < sizeof logp)
		strcpy(logp, l);
	unsetenv("LD_PRELOAD");
	unsetenv("CUTCLOCK_ARM");
	unsetenv("CUTCLOCK_LOG");
}

int
clock_gettime(clockid_t clk, struct timespec *ts)
{
	FILE *f;
	long j;
	int rc, e;

	if (real_cg == NULL)
		real_cg = (int (*)(clockid_t, struct timespec *))dlsym(RTLD_NEXT,
		    "clock_gettime");
	rc = real_cg(clk, ts);
	if (rc != 0 || !on || clk != CLOCK_MONOTONIC)
		return rc;
	e = errno;
	if ((f = fopen(armp, "r")) != NULL) {
		if (fscanf(f, "%ld", &j) != 1)
			j = 0;
		fclose(f);
		unlink(armp);
		target = j;
		n = 0;
	}
	if (target > 0) {
		n++;
		if (logp[0] != '\0' && (f = fopen(logp, "a")) != NULL) {
			fprintf(f, "%ld\n", n);
			fclose(f);
		}
		if (n == target) {
			jump++;
			target = 0;
		}
	}
	ts->tv_sec += jump;
	errno = e;
	return rc;
}
