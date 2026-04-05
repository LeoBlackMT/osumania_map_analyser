export function keysOnLeftHand(keymode) {
    switch (keymode) {
        case 3:
        case 4:
            return 2;
        case 5:
        case 6:
            return 3;
        case 7:
        case 8:
            return 4;
        case 9:
        case 10:
            return 5;
        default:
            throw new Error(`Invalid keymode ${keymode}`);
    }
}
