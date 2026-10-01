// Wuhuicl.Tool - 无界面入口：跑自检、离线验证下载/组装链路。
if (args.Length > 0 && args[0] == "--selftest")
    return await Wuhuicl.Core.SelfTest.RunAsync(Console.Out);
Console.WriteLine("用法: wuhuicl-tool --selftest");
return 0;
