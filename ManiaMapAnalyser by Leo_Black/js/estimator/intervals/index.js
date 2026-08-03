import { rc4K } from "./4k-rc.js";
import { rcExt4K } from "./4k-rc-ext.js";
import { ln4K } from "./4k-ln.js";
import { lnExt4K } from "./4k-ln-ext.js";
import { rc6K } from "./6k-rc.js";
import { ln6K } from "./6k-ln.js";
import { rc7K } from "./7k-rc.js";
import { rcExt7K } from "./7k-rc-ext.js";
import { ln7K } from "./7k-ln.js";
import { rc10K } from "./10k-rc.js";
import { wild7K } from "./7k-wild.js";

export const DAN_INDEX = {
    4: {
        RC: { default: rc4K , extended: rcExt4K },
        LN: { default: ln4K, extended: lnExt4K },
    },
    6: {
        RC: { default: rc6K },
        LN: { default: ln6K },
    },
    7: {
        RC: { default: rc7K, extended: rcExt7K },
        LN: { default: ln7K },
    },
    10: {
        RC: { default: rc10K },
    }
};
