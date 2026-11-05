import sys

from .dates import parse_due
from .discounts import stack_offers
from .money import add_vat


def main(argv=sys.argv[1:]):
    cmd, *rest = argv
    if cmd == "total":
        net, vat = float(rest[1]), float(rest[3])
        print(add_vat(net, vat))
    elif cmd == "quote":
        ...
    elif cmd == "import":
        for row in open(rest[0]).read().splitlines()[1:]:
            parse_due(row.split(",")[3])
