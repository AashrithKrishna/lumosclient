package dev.lumosclient.hud.core;

/**
 * LumosHUD's own 9×9 pixel-art icons for the "Lunar" style, drawn as filled pixels (no texture,
 * so nothing depends on the version's resource-location class). Each sprite is rows of palette
 * characters: {@code .} clear, {@code #} the module's text colour, {@code d} that colour faded,
 * {@code a} the theme's accent, and fixed colours {@code Y} yellow, {@code G} green, {@code O}
 * orange, {@code R} red (see {@link #color}).
 */
public final class Icons {
    private Icons() {}

    public static final int SIZE = 9;

    public static final char[][] FPS = sprite(
        "#########",
        "#.......#",
        "#.....a.#",
        "#.a..a..#",
        "#..aa...#",
        "#.......#",
        "#########",
        "...###...",
        "..#####..");
    public static final char[][] CPS = sprite(
        "..#####..",
        ".#aa#..#.",
        ".#aa#..#.",
        ".#aa#..#.",
        ".#######.",
        ".#.....#.",
        ".#.....#.",
        ".#.....#.",
        "..#####..");
    public static final char[][] COORDS = sprite(
        "..#####..",
        ".#.....#.",
        "#..aaa..#",
        "#..a.a..#",
        "#..aaa..#",
        ".#.....#.",
        "..#...#..",
        "...#.#...",
        "....#....");
    public static final char[][] BIOME = sprite(
        "....a....",
        "...aaa...",
        "..aaaaa..",
        "...aaa...",
        "..aaaaa..",
        ".aaaaaaa.",
        "....#....",
        "....#....",
        "..#####..");
    public static final char[][] CLOCK = sprite(
        "..#####..",
        ".#..#..#.",
        "#...#...#",
        "#...#...#",
        "#...aaa.#",
        "#.......#",
        "#.......#",
        ".#.....#.",
        "..#####..");
    public static final char[][] SERVER = sprite(
        "..#####..",
        ".#..a..#.",
        "#..a.a..#",
        "#aaaaaaa#",
        "#..a.a..#",
        "#aaaaaaa#",
        "#..a.a..#",
        ".#..a..#.",
        "..#####..");
    public static final char[][] SUN = sprite(
        "....Y....",
        ".Y.....Y.",
        "...YYY...",
        "..YYYYY..",
        "Y.YYYYY.Y",
        "..YYYYY..",
        "...YYY...",
        ".Y.....Y.",
        "....Y....");
    public static final char[][] MOON = sprite(
        "..####...",
        ".##d.....",
        "##d......",
        "##d......",
        "##d......",
        "##d......",
        "##d......",
        ".##d.....",
        "..####...");
    public static final char[][] SPEED = sprite(
        ".........",
        ".....#...",
        ".....##..",
        "dd.#####.",
        "...######",
        "dd.#####.",
        ".....##..",
        ".....#...",
        ".........");
    public static final char[][] REACH = sprite(
        ".......##",
        "......#a#",
        ".....#a#.",
        "....#a#..",
        ".#.#a#...",
        "..#a#....",
        "..##.....",
        ".#..#....",
        "#........");
    public static final char[][] COMBO = sprite(
        "....a....",
        "...aa....",
        "...aaa.a.",
        "..aaaaaa.",
        ".aaaYaaa.",
        ".aaYYYaa.",
        ".aaYYYaa.",
        "..aaYaa..",
        "...aaa...");
    public static final char[][] MEMORY = sprite(
        "..#.#.#..",
        ".#######.",
        "##.....##",
        ".#.aaa.#.",
        "##.aaa.##",
        ".#.aaa.#.",
        "##.....##",
        ".#######.",
        "..#.#.#..");
    public static final char[][] SPRINT = sprite(
        ".....aa..",
        ".....aa..",
        "...###...",
        "..#.##.#.",
        "....##...",
        "...#..#..",
        "..#....#.",
        ".#.....#.",
        ".........");
    public static final char[][] SNEAK = sprite(
        ".........",
        ".........",
        "......aa.",
        "....#.aa.",
        "...####..",
        "..#.##...",
        "....###..",
        "...#...#.",
        "..##...##");
    public static final char[][] POTION = sprite(
        "...###...",
        "....#....",
        "....#....",
        "...#.#...",
        "..#...#..",
        ".#aaaaa#.",
        ".#aaaaa#.",
        ".#aaaaa#.",
        "..#####..");

    private static final String[] RING = {
        "...ddd...",
        ".dd...dd.",
        ".d.....d.",
        "d.......d",
        "d.......d",
        "d.......d",
        ".d.....d.",
        ".dd...dd.",
        "...ddd...",
    };

    static char[][] sprite(String... rows) {
        if (rows.length != SIZE) throw new IllegalArgumentException("icons are " + SIZE + " rows");
        char[][] g = new char[SIZE][];
        for (int i = 0; i < SIZE; i++) {
            if (rows[i].length() != SIZE) throw new IllegalArgumentException("icons are " + SIZE + " wide");
            g[i] = rows[i].toCharArray();
        }
        return g;
    }

    /**
     * A compass whose needle points the way the player faces, north up (as on a map).
     * Minecraft yaw: 0 = south (+Z, down), 90 = west (-X, left), 180 = north.
     */
    public static char[][] compass(float yaw) {
        char[][] g = sprite(RING);
        double r = Math.toRadians(yaw), dx = -Math.sin(r), dy = Math.cos(r);
        for (int i = 1; i <= 4; i++) {
            int x = (int) Math.round(4 + dx * i * 0.85), y = (int) Math.round(4 + dy * i * 0.85);
            if (x >= 0 && x < SIZE && y >= 0 && y < SIZE) g[y][x] = 'a';
        }
        for (int i = 1; i <= 2; i++) {
            int x = (int) Math.round(4 - dx * i * 0.85), y = (int) Math.round(4 - dy * i * 0.85);
            if (g[y][x] == '.') g[y][x] = '#';
        }
        g[4][4] = '#';
        return g;
    }

    /** Signal bars for a latency: how many of 4 are lit, and their colour. */
    public static int signalBars(int ms) {
        return ms < 0 ? 0 : ms <= 75 ? 4 : ms <= 150 ? 3 : ms <= 300 ? 2 : 1;
    }

    /** Four bars (1 px wide, 1 px apart) rising left to right; lit ones in green/yellow/orange/red. */
    public static char[][] signal(int ms) {
        int lit = signalBars(ms);
        char c = switch (lit) { case 4 -> 'G'; case 3 -> 'Y'; case 2 -> 'O'; default -> 'R'; };
        char[][] g = new char[SIZE][SIZE];
        for (char[] row : g) java.util.Arrays.fill(row, '.');
        for (int b = 0; b < 4; b++) {
            int x = 1 + b * 2, h = 3 + b * 2;
            for (int y = SIZE - h; y < SIZE; y++) g[y][x] = b < lit ? c : 'd';
        }
        return g;
    }

    /** ARGB of a fixed palette character, or 0 for the ones the caller colours ({@code # d a}) and clear. */
    public static int color(char c) {
        return switch (c) {
            case 'Y' -> 0xFFFFD34A;
            case 'G' -> 0xFF5ADC6E;
            case 'O' -> 0xFFFF9A3C;
            case 'R' -> 0xFFFF5050;
            default -> 0;
        };
    }
}
