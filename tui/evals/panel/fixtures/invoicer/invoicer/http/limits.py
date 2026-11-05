import time


class TokenBucket:
    def __init__(self, per_minute):
        self.per_minute = per_minute
        self.tokens = per_minute
        self.at = time.monotonic()

    def take(self):
        now = time.monotonic()
        self.tokens = min(self.per_minute, self.tokens + (now - self.at) * self.per_minute / 60)
        self.at = now
        if self.tokens < 1:
            return False
        self.tokens -= 1
        return True


def throttled(bucket):
    def wrap(handler):
        def run(req, *args):
            if not bucket.take():
                return 429, "slow down"
            return handler(req, *args)

        return run

    return wrap
