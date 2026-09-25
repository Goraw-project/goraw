using System;
using System.Runtime.InteropServices;

namespace Goraw.Box
{
    public class JobSnapshot
    {
        public bool Success;
        public int ExitCode;
        public double ElapsedSeconds;
        public double CpuTotalSeconds;
        public double CpuUserSeconds;
        public double CpuKernelSeconds;
        public ulong PeakJobMemoryBytes;
        public ulong PeakProcessMemoryBytes;
        public ulong MemoryLimitBytes;
        public uint TotalProcesses;
        public uint PageFaultCount;
        public ulong ReadBytes;
        public ulong WriteBytes;

        public double PeakJobMemoryMB { get { return (double)PeakJobMemoryBytes / (1024.0 * 1024.0); } }
        public double PeakProcessMemoryMB { get { return (double)PeakProcessMemoryBytes / (1024.0 * 1024.0); } }
        public double MemoryLimitMB { get { return (double)MemoryLimitBytes / (1024.0 * 1024.0); } }
        public double MemoryUsagePercent { get { return MemoryLimitBytes > 0 ? ((double)PeakJobMemoryBytes / (double)MemoryLimitBytes) * 100.0 : 0.0; } }
        public double ReadMB { get { return (double)ReadBytes / (1024.0 * 1024.0); } }
        public double WriteMB { get { return (double)WriteBytes / (1024.0 * 1024.0); } }
    }

    public static class JobHelper
    {
        private const uint JOB_OBJECT_ALL_ACCESS = 0x1F001F;
        private const uint JOB_OBJECT_QUERY = 0x0004;

        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Auto)]
        public static extern IntPtr CreateJobObject(IntPtr lpJobAttributes, string lpName);

        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Auto)]
        public static extern IntPtr OpenJobObject(uint dwDesiredAccess, bool bInheritHandles, string lpName);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool QueryInformationJobObject(IntPtr hJob, int JobObjectInformationClass, IntPtr lpJobObjectInfo, uint cbJobObjectInfoLength, out uint lpReturnLength);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool CloseHandle(IntPtr hObject);

        [StructLayout(LayoutKind.Sequential)]
        public struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION
        {
            public long TotalUserTime;
            public long TotalKernelTime;
            public long ThisPeriodTotalUserTime;
            public long ThisPeriodTotalKernelTime;
            public uint TotalPageFaultCount;
            public uint TotalProcesses;
            public uint ActiveProcesses;
            public uint TotalTerminatedProcesses;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct IO_COUNTERS
        {
            public ulong ReadOperationCount;
            public ulong WriteOperationCount;
            public ulong OtherOperationCount;
            public ulong ReadTransferCount;
            public ulong WriteTransferCount;
            public ulong OtherTransferCount;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct JOBOBJECT_BASIC_LIMIT_INFORMATION
        {
            public long PerProcessUserTimeLimit;
            public long PerJobUserTimeLimit;
            public uint LimitFlags;
            public UIntPtr MinimumWorkingSetSize;
            public UIntPtr MaximumWorkingSetSize;
            public uint ActiveProcessLimit;
            public UIntPtr Affinity;
            public uint PriorityClass;
            public uint SchedulingClass;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION
        {
            public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
            public IO_COUNTERS IoInfo;
            public UIntPtr ProcessMemoryLimit;
            public UIntPtr JobMemoryLimit;
            public UIntPtr PeakProcessMemoryUsed;
            public UIntPtr PeakJobMemoryUsed;
        }

        public static IntPtr CreateBoxJob(string jobName)
        {
            return CreateJobObject(IntPtr.Zero, jobName);
        }

        public static JobSnapshot QueryJob(IntPtr hJob)
        {
            JobSnapshot snap = new JobSnapshot();
            if (hJob == IntPtr.Zero) return snap;

            int extSize = Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION));
            IntPtr extPtr = Marshal.AllocHGlobal(extSize);
            try
            {
                uint retLen;
                if (QueryInformationJobObject(hJob, 9, extPtr, (uint)extSize, out retLen))
                {
                    JOBOBJECT_EXTENDED_LIMIT_INFORMATION ext = (JOBOBJECT_EXTENDED_LIMIT_INFORMATION)Marshal.PtrToStructure(extPtr, typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION));
                    snap.PeakJobMemoryBytes = ext.PeakJobMemoryUsed.ToUInt64();
                    snap.PeakProcessMemoryBytes = ext.PeakProcessMemoryUsed.ToUInt64();
                    snap.MemoryLimitBytes = ext.ProcessMemoryLimit.ToUInt64();
                    if (snap.MemoryLimitBytes == 0)
                    {
                        snap.MemoryLimitBytes = ext.JobMemoryLimit.ToUInt64();
                    }
                    snap.ReadBytes = ext.IoInfo.ReadTransferCount;
                    snap.WriteBytes = ext.IoInfo.WriteTransferCount;
                    snap.Success = true;
                }
            }
            finally
            {
                Marshal.FreeHGlobal(extPtr);
            }

            int accSize = Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION));
            IntPtr accPtr = Marshal.AllocHGlobal(accSize);
            try
            {
                uint retLen;
                if (QueryInformationJobObject(hJob, 1, accPtr, (uint)accSize, out retLen))
                {
                    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION acc = (JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)Marshal.PtrToStructure(accPtr, typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION));
                    snap.TotalProcesses = acc.TotalProcesses;
                    snap.PageFaultCount = acc.TotalPageFaultCount;
                    snap.CpuUserSeconds = (double)acc.TotalUserTime / 10000000.0;
                    snap.CpuKernelSeconds = (double)acc.TotalKernelTime / 10000000.0;
                    snap.CpuTotalSeconds = snap.CpuUserSeconds + snap.CpuKernelSeconds;
                }
            }
            finally
            {
                Marshal.FreeHGlobal(accPtr);
            }

            return snap;
        }

        public static void CloseBoxJob(IntPtr hJob)
        {
            if (hJob != IntPtr.Zero)
            {
                CloseHandle(hJob);
            }
        }
    }
}
