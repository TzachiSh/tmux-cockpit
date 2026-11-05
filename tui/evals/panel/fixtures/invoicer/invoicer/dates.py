from datetime import datetime

DUE_FORMAT = "%d/%m/%Y"


def parse_due(text):
    """The due date column of an orders file."""
    return datetime.strptime(text.strip(), DUE_FORMAT).date()


def days_left(due, today):
    return (due - today).days
