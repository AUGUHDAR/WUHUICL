using System;
using System.Collections.Generic;

namespace Wuhuicl.App.Services;

/// <summary>
/// 开机碎玻璃动画的纯算法层，逐函数移植自 KAMUCL-1.1.5 的 src/shared/startup.ts。
/// 移植而非重写的原因：抖动三角剖分依赖同一个 LCG 种子（1041）与同一套常数，
/// 换一种随机法就拼不出同一张脸。
/// </summary>
public sealed class GlassPoint
{
    public double X, Y;
    public GlassPoint() { }
    public GlassPoint(double x, double y) { X = x; Y = y; }
}

public sealed class GlassShard
{
    public List<GlassPoint> Vertices = new();
    public double SourceX, SourceY, TargetX, TargetY;
    public double X, Y, Phase, Spin, Delay;
    public double OffsetX, OffsetY, VelocityX, VelocityY;
    public (double x, double y, double rot, double scale)? From;
}

public sealed class GlassGeometry
{
    public double Board;
    public double Left, Top;
    public List<GlassShard> Shards = new();
}

public static class BootGlass
{
    public const double GlassFloatMinMs = 900;
    public const double ConvergeDuration = 1400;
    public const double AssembledHoldMs = 2000;
    public const double BootFrameMs = 1000.0 / 60;
    public const double GlassPointerRadius = 140;

    static double Clamp(double n) => Math.Max(0, Math.Min(1, n));

    /// <summary>smootherstep：t³t²(6t-15)+10，原版的汇聚缓动。</summary>
    static double Smooth(double n)
    {
        var t = Clamp(n);
        return t * t * t * (t * (t * 6 - 15) + 10);
    }

    /// <summary>6×6 抖动网格切成 72 个三角碎片；seed 与 LCG 常数必须与原版一致。</summary>
    public static GlassGeometry Make(double width, double height)
    {
        uint seed = 1041;
        double Random()
        {
            seed = (uint)((seed * 1664525u + 1013904223u) & 0xFFFFFFFF);
            return seed / 4294967296.0;
        }

        var board = Math.Max(80, Math.Floor(Math.Min(224, Math.Min(width * .26, height * .32))));
        var left = Math.Round((width - board) / 2);
        var top = Math.Round((height - board) / 2);
        var cell = board / 6;

        var points = new List<GlassPoint>();
        for (var row = 0; row <= 6; row++)
            for (var col = 0; col <= 6; col++)
                points.Add(new GlassPoint(
                    (col + (col > 0 && col < 6 ? (Random() - .5) * .65 : 0)) * cell,
                    (row + (row > 0 && row < 6 ? (Random() - .5) * .65 : 0)) * cell));

        var g = new GlassGeometry { Board = board, Left = left, Top = top };

        void Add(List<GlassPoint> v)
        {
            double sx = 0, sy = 0;
            foreach (var p in v) { sx += p.X; sy += p.Y; }
            sx /= 3; sy /= 3;
            var shard = new GlassShard
            {
                SourceX = sx,
                SourceY = sy,
                TargetX = left + sx,
                TargetY = top + sy,
                X = width * (.10 + Random() * .80),
                Y = height * (.14 + Random() * .62),
                Phase = Random() * Math.PI * 2,
                Spin = (Random() - .5) * 2.4,
                Delay = Random() * 180,
            };
            foreach (var p in v) shard.Vertices.Add(new GlassPoint(p.X - sx, p.Y - sy));
            g.Shards.Add(shard);
        }

        for (var row = 0; row < 6; row++)
            for (var col = 0; col < 6; col++)
            {
                var a = points[row * 7 + col];
                var b = points[row * 7 + col + 1];
                var c = points[(row + 1) * 7 + col];
                var d = points[(row + 1) * 7 + col + 1];
                if ((row + col) % 2 == 1) { Add(new List<GlassPoint> { a, b, c }); Add(new List<GlassPoint> { b, d, c }); }
                else { Add(new List<GlassPoint> { a, b, d }); Add(new List<GlassPoint> { a, d, c }); }
            }
        return g;
    }

    static (double x, double y, double rot, double scale) Floating(GlassShard s, double elapsed) =>
        (s.X + Math.Sin(elapsed / 1500 + s.Phase) * 14 + s.OffsetX,
         s.Y + Math.Cos(elapsed / 1800 + s.Phase) * 11 + s.OffsetY,
         s.Spin + Math.Sin(elapsed / 2100 + s.Phase) * .22,
         1.06 + Math.Sin(elapsed / 1900 + s.Phase) * .12);

    /// <summary>指针附近的有界弹簧扰动；converge 前每帧调用。</summary>
    public static void Advance(GlassGeometry g, double elapsed, double deltaMs, GlassPoint? pointer)
    {
        var dt = Math.Min(32, Math.Max(0, deltaMs)) / 1000;
        foreach (var s in g.Shards)
        {
            var p = Floating(s, elapsed);
            double fx = 0, fy = 0;
            if (pointer is not null)
            {
                var dx = p.x - pointer.X;
                var dy = p.y - pointer.Y;
                var distance = Math.Sqrt(dx * dx + dy * dy);
                if (distance < GlassPointerRadius)
                {
                    if (distance < 1) { dx = Math.Cos(s.Phase); dy = Math.Sin(s.Phase); }
                    var k = 1 - distance / GlassPointerRadius;
                    var force = 2400 * k * k / Math.Max(1, distance);
                    fx = dx * force; fy = dy * force;
                }
            }
            var drag = Math.Exp(-5 * dt);
            s.VelocityX = (s.VelocityX + (fx - s.OffsetX * 8) * dt) * drag;
            s.VelocityY = (s.VelocityY + (fy - s.OffsetY * 8) * dt) * drag;
            s.OffsetX = Math.Max(-180, Math.Min(180, s.OffsetX + s.VelocityX * dt));
            s.OffsetY = Math.Max(-180, Math.Min(180, s.OffsetY + s.VelocityY * dt));
        }
    }

    /// <summary>某块在给定时刻的落位；convergeAt 为 null 表示还在漂浮。</summary>
    public static (double x, double y, double rot, double scale, double progress) Pose(
        GlassShard s, double elapsed, double? convergeAt)
    {
        if (convergeAt is null)
        {
            var f = Floating(s, elapsed);
            return (f.x, f.y, f.rot, f.scale, 0);
        }
        s.From ??= Floating(s, convergeAt.Value);
        var at = s.From.Value;
        var progress = Smooth((elapsed - convergeAt.Value - s.Delay) / (ConvergeDuration - 180));
        if (progress == 1) return (s.TargetX, s.TargetY, 0, 1, 1);
        return (at.x + (s.TargetX - at.x) * progress,
                at.y + (s.TargetY - at.y) * progress,
                at.rot * (1 - progress),
                at.scale + (1 - at.scale) * progress,
                progress);
    }
}
