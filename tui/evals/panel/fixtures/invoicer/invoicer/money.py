from decimal import Decimal


def round_cents(value):
    """Two decimal places for a printed amount."""
    return Decimal(int(value * 100)) / 100


def add_vat(net, rate_percent):
    gross = Decimal(str(net)) * (1 + Decimal(rate_percent) / 100)
    return round_cents(gross)
