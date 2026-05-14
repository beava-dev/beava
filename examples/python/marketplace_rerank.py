"""
Marketplace reranking: reorder the marketplace while shoppers are still
shopping.

Tracks per-SKU momentum (cart velocity, distinct buyers, last view) and
per-user intent (average view price, top category, recommendation
fatigue), then prints which reranking reflex would fire.

Run:

    beava --data-dir ./.beava/
    python3 examples/python/marketplace_rerank.py
"""
import beava as bv


@bv.event
class ShopEvent:
    user_id: str
    sku: str
    category: str
    price: float
    action: str   # "view" | "add_to_cart" | "purchase"


@bv.table(key="sku")
def SkuMomentum(e: ShopEvent):
    return e.group_by("sku").agg(
        cart_velocity_5m  = bv.count(window="5m", where=bv.col("action") == "add_to_cart"),
        views_30m         = bv.count(window="30m", where=bv.col("action") == "view"),
        unique_buyers_1h  = bv.n_unique("user_id", window="1h", where=bv.col("action") == "purchase"),
        last_action_at    = bv.last_seen(),
    )


@bv.table(key="user_id")
def UserIntent(e: ShopEvent):
    return e.group_by("user_id").agg(
        avg_view_price_30m = bv.mean("price", window="30m", where=bv.col("action") == "view"),
        top_category_30m   = bv.top_k("category", k=1, window="30m"),
        unique_skus_30m    = bv.n_unique("sku", window="30m"),
        cart_intent_10m    = bv.count(window="10m", where=bv.col("action") == "add_to_cart"),
    )


def main() -> int:
    app = bv.App("http://localhost:8080")
    app.register(ShopEvent, SkuMomentum, UserIntent, force=True)

    # A bursty browse-then-buy session for user_1382, with sku_882 trending.
    flows = [
        ("user_1382", "sku_882", "watches",  220.0, "view"),
        ("user_1382", "sku_882", "watches",  220.0, "add_to_cart"),
        ("user_1382", "sku_910", "watches",  340.0, "view"),
        ("user_1382", "sku_882", "watches",  220.0, "purchase"),
        ("user_2014", "sku_882", "watches",  220.0, "view"),
        ("user_2014", "sku_882", "watches",  220.0, "add_to_cart"),
        ("user_3122", "sku_882", "watches",  220.0, "add_to_cart"),
        ("user_1382", "sku_445", "shoes",     95.0, "view"),
    ]
    for user, sku, cat, price, action in flows:
        app.push("ShopEvent", {
            "user_id": user, "sku": sku, "category": cat,
            "price":   price, "action":  action,
        })

    sku = app.get("SkuMomentum", "sku_882")
    user = app.get("UserIntent", "user_1382")
    print(f"sku_882 momentum:    {sku}")
    print(f"user_1382 intent:    {user}")

    if (sku.get("cart_velocity_5m") or 0) >= 2:
        print("reflex: boost sku_882 in trending shelf")
    if (user.get("avg_view_price_30m") or 0) > 200:
        print("reflex: sort user_1382 toward premium picks")
    if (user.get("unique_skus_30m") or 0) >= 3:
        print("reflex: diversify recommendations for user_1382")

    print("OK -- marketplace_rerank.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
