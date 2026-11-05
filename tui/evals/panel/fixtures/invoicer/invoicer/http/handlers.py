from .limits import TokenBucket, throttled

READS = TokenBucket(per_minute=120)


@throttled(READS)
def get_invoice(req, invoice_id):
    return 200, req.store.load(invoice_id)


def post_invoice(req):
    invoice = req.store.create(req.json())
    return 201, invoice
