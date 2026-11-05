from invoicer.money import add_vat


def test_add_vat_whole():
    assert str(add_vat(100, 19)) == "119"
