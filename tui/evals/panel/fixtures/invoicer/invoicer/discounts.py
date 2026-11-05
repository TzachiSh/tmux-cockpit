LOYALTY_RATE = 0.05


def loyalty_rebate(customer, subtotal):
    if customer.years < 2:
        return 0
    return round(subtotal * LOYALTY_RATE, 2)


def coupon_value(code, subtotal):
    return round(subtotal * 0.10, 2) if code == "SPRING10" else 0


def stack_offers(order):
    lines = [("loyalty", loyalty_rebate(order.customer, order.subtotal))]
    if order.coupon:
        lines.append(("loyalty", loyalty_rebate(order.customer, order.subtotal)))
        lines.append(("coupon", coupon_value(order.coupon, order.subtotal)))
    return lines
