; Goraw -> LLVM IR
target triple = "x86_64-w64-windows-gnu"

%slice = type { ptr, i64 }

%struct.Point = type { i64, i64, i64 }
%struct.Edge = type { i64, i64, i64 }
%struct.BufferGuard = type { ptr }

declare i32 @printf(ptr, ...)
declare i64 @clock()
declare ptr @malloc(i64)
declare void @free(ptr)

@.str.0 = private unnamed_addr constant [57 x i8] c"[ERROR] Coordinates out of supported range [%lld, %lld]\0A\00"
@.str.1 = private unnamed_addr constant [59 x i8] c"Running benchmark on N = %lld points (Hard constraint)...\0A\00"
@.str.2 = private unnamed_addr constant [74 x i8] c"100,000 points MST computed successfully: Total weight = %lld in %lld ms\0A\00"
@.str.3 = private unnamed_addr constant [62 x i8] c"============================================================\0A\00"
@.str.4 = private unnamed_addr constant [52 x i8] c"  Manhattan MST - O(N log N) Algorithm in Goraw   \0A\00"
@.str.5 = private unnamed_addr constant [63 x i8] c"============================================================\0A\0A\00"
@.str.6 = private unnamed_addr constant [42 x i8] c"Sample 3 points: MST = %lld (Expected 5)\0A\00"
@.str.7 = private unnamed_addr constant [50 x i8] c"Sample square 4 points: MST = %lld (Expected 30)\0A\00"
@.str.8 = private unnamed_addr constant [46 x i8] c"\0ADone! Manhattan MST completed successfully.\0A\00"

define %struct.BufferGuard @BufferGuard__new(i64 %arg.sz) alwaysinline {
entry:
  %sz.1 = alloca i64
  %p.2 = alloca ptr
  %lit_BufferGuard.3 = alloca %struct.BufferGuard
  store i64 %arg.sz, ptr %sz.1
  %t1 = load i64, ptr %sz.1
  %t2 = call ptr @malloc(i64 %t1)
  store ptr %t2, ptr %p.2
  %t3 = load ptr, ptr %p.2
  %t4 = getelementptr %struct.BufferGuard, ptr %lit_BufferGuard.3, i32 0, i32 0
  store ptr %t3, ptr %t4
  %t5 = load %struct.BufferGuard, ptr %lit_BufferGuard.3
  ret %struct.BufferGuard %t5
}

define void @BufferGuard__drop(ptr %arg.self) alwaysinline {
entry:
  %self.1 = alloca ptr
  store ptr %arg.self, ptr %self.1
  %t1 = load ptr, ptr %self.1
  %t2 = getelementptr %struct.BufferGuard, ptr %t1, i32 0, i32 0
  %t3 = load ptr, ptr %t2
  %t4 = icmp ne ptr %t3, null
  br i1 %t4, label %then1, label %endif2
then1:
  %t5 = load ptr, ptr %self.1
  %t6 = getelementptr %struct.BufferGuard, ptr %t5, i32 0, i32 0
  %t7 = load ptr, ptr %t6
  call void @free(ptr %t7)
  %t8 = load ptr, ptr %self.1
  %t9 = getelementptr %struct.BufferGuard, ptr %t8, i32 0, i32 0
  store ptr null, ptr %t9
  br label %endif2
endif2:
  ret void
}

define i64 @abs_i64(i64 %arg.x) alwaysinline {
entry:
  %x.1 = alloca i64
  store i64 %arg.x, ptr %x.1
  %t1 = load i64, ptr %x.1
  %t2 = icmp sge i64 %t1, 0
  br i1 %t2, label %then1, label %endif2
then1:
  %t3 = load i64, ptr %x.1
  ret i64 %t3
endif2:
  %t4 = load i64, ptr %x.1
  %t5 = sub i64 0, 9223372036854775807
  %t6 = sub i64 %t5, 1
  %t7 = icmp eq i64 %t4, %t6
  br i1 %t7, label %then3, label %endif4
then3:
  ret i64 9223372036854775807
endif4:
  %t8 = load i64, ptr %x.1
  %t9 = sub i64 0, %t8
  ret i64 %t9
}

define i1 @validate_coordinates(ptr %arg.orig_x, ptr %arg.orig_y, i64 %arg.n) alwaysinline {
entry:
  %orig_x.1 = alloca ptr
  %orig_y.2 = alloca ptr
  %n.3 = alloca i64
  %i.4 = alloca i64
  %logic.5 = alloca i1
  %logic.6 = alloca i1
  store ptr %arg.orig_x, ptr %orig_x.1
  store ptr %arg.orig_y, ptr %orig_y.2
  store i64 %arg.n, ptr %n.3
  store i64 0, ptr %i.4
  br label %fcond1
fcond1:
  %t1 = load i64, ptr %i.4
  %t2 = load i64, ptr %n.3
  %t3 = icmp slt i64 %t1, %t2
  br i1 %t3, label %fbody2, label %fend4
fbody2:
  %t4 = load ptr, ptr %orig_x.1
  %t5 = load i64, ptr %i.4
  %t6 = getelementptr i64, ptr %t4, i64 %t5
  %t7 = load i64, ptr %t6
  %t8 = icmp slt i64 %t7, -1000000000
  store i1 %t8, ptr %logic.5
  br i1 %t8, label %logdone6, label %logrhs5
logrhs5:
  %t9 = load ptr, ptr %orig_x.1
  %t10 = load i64, ptr %i.4
  %t11 = getelementptr i64, ptr %t9, i64 %t10
  %t12 = load i64, ptr %t11
  %t13 = icmp sgt i64 %t12, 1000000000
  store i1 %t13, ptr %logic.5
  br label %logdone6
logdone6:
  %t14 = load i1, ptr %logic.5
  br i1 %t14, label %then7, label %endif8
then7:
  ret i1 false
endif8:
  %t15 = load ptr, ptr %orig_y.2
  %t16 = load i64, ptr %i.4
  %t17 = getelementptr i64, ptr %t15, i64 %t16
  %t18 = load i64, ptr %t17
  %t19 = icmp slt i64 %t18, -1000000000
  store i1 %t19, ptr %logic.6
  br i1 %t19, label %logdone10, label %logrhs9
logrhs9:
  %t20 = load ptr, ptr %orig_y.2
  %t21 = load i64, ptr %i.4
  %t22 = getelementptr i64, ptr %t20, i64 %t21
  %t23 = load i64, ptr %t22
  %t24 = icmp sgt i64 %t23, 1000000000
  store i1 %t24, ptr %logic.6
  br label %logdone10
logdone10:
  %t25 = load i1, ptr %logic.6
  br i1 %t25, label %then11, label %endif12
then11:
  ret i1 false
endif12:
  br label %fpost3
fpost3:
  %t26 = load i64, ptr %i.4
  %t27 = add i64 %t26, 1
  store i64 %t27, ptr %i.4
  br label %fcond1
fend4:
  ret i1 true
}

define i1 @point_greater(i64 %arg.p1_x, i64 %arg.p1_y, i64 %arg.p2_x, i64 %arg.p2_y) alwaysinline {
entry:
  %p1_x.1 = alloca i64
  %p1_y.2 = alloca i64
  %p2_x.3 = alloca i64
  %p2_y.4 = alloca i64
  store i64 %arg.p1_x, ptr %p1_x.1
  store i64 %arg.p1_y, ptr %p1_y.2
  store i64 %arg.p2_x, ptr %p2_x.3
  store i64 %arg.p2_y, ptr %p2_y.4
  %t1 = load i64, ptr %p1_x.1
  %t2 = load i64, ptr %p2_x.3
  %t3 = icmp ne i64 %t1, %t2
  br i1 %t3, label %then1, label %endif2
then1:
  %t4 = load i64, ptr %p1_x.1
  %t5 = load i64, ptr %p2_x.3
  %t6 = icmp sgt i64 %t4, %t5
  ret i1 %t6
endif2:
  %t7 = load i64, ptr %p1_y.2
  %t8 = load i64, ptr %p2_y.4
  %t9 = icmp sgt i64 %t7, %t8
  ret i1 %t9
}

define i64 @calc_max_depth(i64 %arg.n) alwaysinline {
entry:
  %n.1 = alloca i64
  %d.2 = alloca i64
  %v.3 = alloca i64
  store i64 %arg.n, ptr %n.1
  store i64 0, ptr %d.2
  %t1 = load i64, ptr %n.1
  store i64 %t1, ptr %v.3
  br label %fcond1
fcond1:
  %t2 = load i64, ptr %v.3
  %t3 = icmp sgt i64 %t2, 0
  br i1 %t3, label %fbody2, label %fend4
fbody2:
  %t4 = load i64, ptr %d.2
  %t5 = add i64 %t4, 1
  store i64 %t5, ptr %d.2
  %t6 = load i64, ptr %v.3
  %t7 = sdiv i64 %t6, 2
  store i64 %t7, ptr %v.3
  br label %fpost3
fpost3:
  br label %fcond1
fend4:
  %t8 = load i64, ptr %d.2
  %t9 = mul i64 %t8, 2
  ret i64 %t9
}

define void @insertion_sort_points(ptr %arg.pts, i64 %arg.left, i64 %arg.right) alwaysinline {
entry:
  %pts.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %i.4 = alloca i64
  %key.5 = alloca %struct.Point
  %j.6 = alloca i64
  %logic.7 = alloca i1
  store ptr %arg.pts, ptr %pts.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  %t1 = load i64, ptr %left.2
  %t2 = add i64 %t1, 1
  store i64 %t2, ptr %i.4
  br label %fcond1
fcond1:
  %t3 = load i64, ptr %i.4
  %t4 = load i64, ptr %right.3
  %t5 = icmp sle i64 %t3, %t4
  br i1 %t5, label %fbody2, label %fend4
fbody2:
  %t6 = load ptr, ptr %pts.1
  %t7 = load i64, ptr %i.4
  %t8 = getelementptr %struct.Point, ptr %t6, i64 %t7
  %t9 = load %struct.Point, ptr %t8
  store %struct.Point %t9, ptr %key.5
  %t10 = load i64, ptr %i.4
  %t11 = sub i64 %t10, 1
  store i64 %t11, ptr %j.6
  br label %fcond5
fcond5:
  %t12 = load i64, ptr %j.6
  %t13 = load i64, ptr %left.2
  %t14 = icmp sge i64 %t12, %t13
  store i1 %t14, ptr %logic.7
  br i1 %t14, label %logrhs9, label %logdone10
logrhs9:
  %t15 = getelementptr %struct.Point, ptr %key.5, i32 0, i32 0
  %t16 = load i64, ptr %t15
  %t17 = getelementptr %struct.Point, ptr %key.5, i32 0, i32 1
  %t18 = load i64, ptr %t17
  %t19 = load ptr, ptr %pts.1
  %t20 = load i64, ptr %j.6
  %t21 = getelementptr %struct.Point, ptr %t19, i64 %t20
  %t22 = getelementptr %struct.Point, ptr %t21, i32 0, i32 0
  %t23 = load i64, ptr %t22
  %t24 = load ptr, ptr %pts.1
  %t25 = load i64, ptr %j.6
  %t26 = getelementptr %struct.Point, ptr %t24, i64 %t25
  %t27 = getelementptr %struct.Point, ptr %t26, i32 0, i32 1
  %t28 = load i64, ptr %t27
  %t29 = call i1 @point_greater(i64 %t16, i64 %t18, i64 %t23, i64 %t28)
  store i1 %t29, ptr %logic.7
  br label %logdone10
logdone10:
  %t30 = load i1, ptr %logic.7
  br i1 %t30, label %fbody6, label %fend8
fbody6:
  %t31 = load ptr, ptr %pts.1
  %t32 = load i64, ptr %j.6
  %t33 = add i64 %t32, 1
  %t34 = getelementptr %struct.Point, ptr %t31, i64 %t33
  %t35 = load ptr, ptr %pts.1
  %t36 = load i64, ptr %j.6
  %t37 = getelementptr %struct.Point, ptr %t35, i64 %t36
  %t38 = load %struct.Point, ptr %t37
  store %struct.Point %t38, ptr %t34
  %t39 = load i64, ptr %j.6
  %t40 = sub i64 %t39, 1
  store i64 %t40, ptr %j.6
  br label %fpost7
fpost7:
  br label %fcond5
fend8:
  %t41 = load ptr, ptr %pts.1
  %t42 = load i64, ptr %j.6
  %t43 = add i64 %t42, 1
  %t44 = getelementptr %struct.Point, ptr %t41, i64 %t43
  %t45 = load %struct.Point, ptr %key.5
  store %struct.Point %t45, ptr %t44
  br label %fpost3
fpost3:
  %t46 = load i64, ptr %i.4
  %t47 = add i64 %t46, 1
  store i64 %t47, ptr %i.4
  br label %fcond1
fend4:
  ret void
}

define void @heap_sift_down_points(ptr %arg.pts, i64 %arg.left, i64 %arg.root, i64 %arg.n) alwaysinline {
entry:
  %pts.1 = alloca ptr
  %left.2 = alloca i64
  %root.3 = alloca i64
  %n.4 = alloca i64
  %curr.5 = alloca i64
  %smallest.6 = alloca i64
  %left_child.7 = alloca i64
  %right_child.8 = alloca i64
  %logic.9 = alloca i1
  %logic.10 = alloca i1
  %tmp.11 = alloca %struct.Point
  store ptr %arg.pts, ptr %pts.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.root, ptr %root.3
  store i64 %arg.n, ptr %n.4
  %t1 = load i64, ptr %root.3
  store i64 %t1, ptr %curr.5
  br label %fcond1
fcond1:
  br label %fbody2
fbody2:
  %t2 = load i64, ptr %curr.5
  store i64 %t2, ptr %smallest.6
  %t3 = load i64, ptr %curr.5
  %t4 = mul i64 2, %t3
  %t5 = add i64 %t4, 1
  store i64 %t5, ptr %left_child.7
  %t6 = load i64, ptr %curr.5
  %t7 = mul i64 2, %t6
  %t8 = add i64 %t7, 2
  store i64 %t8, ptr %right_child.8
  %t9 = load i64, ptr %left_child.7
  %t10 = load i64, ptr %n.4
  %t11 = icmp slt i64 %t9, %t10
  store i1 %t11, ptr %logic.9
  br i1 %t11, label %logrhs5, label %logdone6
logrhs5:
  %t12 = load ptr, ptr %pts.1
  %t13 = load i64, ptr %left.2
  %t14 = load i64, ptr %smallest.6
  %t15 = add i64 %t13, %t14
  %t16 = getelementptr %struct.Point, ptr %t12, i64 %t15
  %t17 = getelementptr %struct.Point, ptr %t16, i32 0, i32 0
  %t18 = load i64, ptr %t17
  %t19 = load ptr, ptr %pts.1
  %t20 = load i64, ptr %left.2
  %t21 = load i64, ptr %smallest.6
  %t22 = add i64 %t20, %t21
  %t23 = getelementptr %struct.Point, ptr %t19, i64 %t22
  %t24 = getelementptr %struct.Point, ptr %t23, i32 0, i32 1
  %t25 = load i64, ptr %t24
  %t26 = load ptr, ptr %pts.1
  %t27 = load i64, ptr %left.2
  %t28 = load i64, ptr %left_child.7
  %t29 = add i64 %t27, %t28
  %t30 = getelementptr %struct.Point, ptr %t26, i64 %t29
  %t31 = getelementptr %struct.Point, ptr %t30, i32 0, i32 0
  %t32 = load i64, ptr %t31
  %t33 = load ptr, ptr %pts.1
  %t34 = load i64, ptr %left.2
  %t35 = load i64, ptr %left_child.7
  %t36 = add i64 %t34, %t35
  %t37 = getelementptr %struct.Point, ptr %t33, i64 %t36
  %t38 = getelementptr %struct.Point, ptr %t37, i32 0, i32 1
  %t39 = load i64, ptr %t38
  %t40 = call i1 @point_greater(i64 %t18, i64 %t25, i64 %t32, i64 %t39)
  store i1 %t40, ptr %logic.9
  br label %logdone6
logdone6:
  %t41 = load i1, ptr %logic.9
  br i1 %t41, label %then7, label %endif8
then7:
  %t42 = load i64, ptr %left_child.7
  store i64 %t42, ptr %smallest.6
  br label %endif8
endif8:
  %t43 = load i64, ptr %right_child.8
  %t44 = load i64, ptr %n.4
  %t45 = icmp slt i64 %t43, %t44
  store i1 %t45, ptr %logic.10
  br i1 %t45, label %logrhs9, label %logdone10
logrhs9:
  %t46 = load ptr, ptr %pts.1
  %t47 = load i64, ptr %left.2
  %t48 = load i64, ptr %smallest.6
  %t49 = add i64 %t47, %t48
  %t50 = getelementptr %struct.Point, ptr %t46, i64 %t49
  %t51 = getelementptr %struct.Point, ptr %t50, i32 0, i32 0
  %t52 = load i64, ptr %t51
  %t53 = load ptr, ptr %pts.1
  %t54 = load i64, ptr %left.2
  %t55 = load i64, ptr %smallest.6
  %t56 = add i64 %t54, %t55
  %t57 = getelementptr %struct.Point, ptr %t53, i64 %t56
  %t58 = getelementptr %struct.Point, ptr %t57, i32 0, i32 1
  %t59 = load i64, ptr %t58
  %t60 = load ptr, ptr %pts.1
  %t61 = load i64, ptr %left.2
  %t62 = load i64, ptr %right_child.8
  %t63 = add i64 %t61, %t62
  %t64 = getelementptr %struct.Point, ptr %t60, i64 %t63
  %t65 = getelementptr %struct.Point, ptr %t64, i32 0, i32 0
  %t66 = load i64, ptr %t65
  %t67 = load ptr, ptr %pts.1
  %t68 = load i64, ptr %left.2
  %t69 = load i64, ptr %right_child.8
  %t70 = add i64 %t68, %t69
  %t71 = getelementptr %struct.Point, ptr %t67, i64 %t70
  %t72 = getelementptr %struct.Point, ptr %t71, i32 0, i32 1
  %t73 = load i64, ptr %t72
  %t74 = call i1 @point_greater(i64 %t52, i64 %t59, i64 %t66, i64 %t73)
  store i1 %t74, ptr %logic.10
  br label %logdone10
logdone10:
  %t75 = load i1, ptr %logic.10
  br i1 %t75, label %then11, label %endif12
then11:
  %t76 = load i64, ptr %right_child.8
  store i64 %t76, ptr %smallest.6
  br label %endif12
endif12:
  %t77 = load i64, ptr %smallest.6
  %t78 = load i64, ptr %curr.5
  %t79 = icmp eq i64 %t77, %t78
  br i1 %t79, label %then13, label %endif14
then13:
  br label %fend4
endif14:
  %t80 = load ptr, ptr %pts.1
  %t81 = load i64, ptr %left.2
  %t82 = load i64, ptr %curr.5
  %t83 = add i64 %t81, %t82
  %t84 = getelementptr %struct.Point, ptr %t80, i64 %t83
  %t85 = load %struct.Point, ptr %t84
  store %struct.Point %t85, ptr %tmp.11
  %t86 = load ptr, ptr %pts.1
  %t87 = load i64, ptr %left.2
  %t88 = load i64, ptr %curr.5
  %t89 = add i64 %t87, %t88
  %t90 = getelementptr %struct.Point, ptr %t86, i64 %t89
  %t91 = load ptr, ptr %pts.1
  %t92 = load i64, ptr %left.2
  %t93 = load i64, ptr %smallest.6
  %t94 = add i64 %t92, %t93
  %t95 = getelementptr %struct.Point, ptr %t91, i64 %t94
  %t96 = load %struct.Point, ptr %t95
  store %struct.Point %t96, ptr %t90
  %t97 = load ptr, ptr %pts.1
  %t98 = load i64, ptr %left.2
  %t99 = load i64, ptr %smallest.6
  %t100 = add i64 %t98, %t99
  %t101 = getelementptr %struct.Point, ptr %t97, i64 %t100
  %t102 = load %struct.Point, ptr %tmp.11
  store %struct.Point %t102, ptr %t101
  %t103 = load i64, ptr %smallest.6
  store i64 %t103, ptr %curr.5
  br label %fpost3
fpost3:
  br label %fcond1
fend4:
  ret void
}

define void @heapsort_points(ptr %arg.pts, i64 %arg.left, i64 %arg.right) alwaysinline {
entry:
  %pts.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %n.4 = alloca i64
  %i.5 = alloca i64
  %j.6 = alloca i64
  %tmp.7 = alloca %struct.Point
  store ptr %arg.pts, ptr %pts.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  %t1 = load i64, ptr %right.3
  %t2 = load i64, ptr %left.2
  %t3 = sub i64 %t1, %t2
  %t4 = add i64 %t3, 1
  store i64 %t4, ptr %n.4
  %t5 = load i64, ptr %n.4
  %t6 = icmp sle i64 %t5, 1
  br i1 %t6, label %then1, label %endif2
then1:
  ret void
endif2:
  %t7 = load i64, ptr %n.4
  %t8 = sdiv i64 %t7, 2
  %t9 = sub i64 %t8, 1
  store i64 %t9, ptr %i.5
  br label %fcond3
fcond3:
  %t10 = load i64, ptr %i.5
  %t11 = icmp sge i64 %t10, 0
  br i1 %t11, label %fbody4, label %fend6
fbody4:
  %t12 = load ptr, ptr %pts.1
  %t13 = load i64, ptr %left.2
  %t14 = load i64, ptr %i.5
  %t15 = load i64, ptr %n.4
  call void @heap_sift_down_points(ptr %t12, i64 %t13, i64 %t14, i64 %t15)
  %t16 = load i64, ptr %i.5
  %t17 = sub i64 %t16, 1
  store i64 %t17, ptr %i.5
  br label %fpost5
fpost5:
  br label %fcond3
fend6:
  %t18 = load i64, ptr %n.4
  %t19 = sub i64 %t18, 1
  store i64 %t19, ptr %j.6
  br label %fcond7
fcond7:
  %t20 = load i64, ptr %j.6
  %t21 = icmp sgt i64 %t20, 0
  br i1 %t21, label %fbody8, label %fend10
fbody8:
  %t22 = load ptr, ptr %pts.1
  %t23 = load i64, ptr %left.2
  %t24 = getelementptr %struct.Point, ptr %t22, i64 %t23
  %t25 = load %struct.Point, ptr %t24
  store %struct.Point %t25, ptr %tmp.7
  %t26 = load ptr, ptr %pts.1
  %t27 = load i64, ptr %left.2
  %t28 = getelementptr %struct.Point, ptr %t26, i64 %t27
  %t29 = load ptr, ptr %pts.1
  %t30 = load i64, ptr %left.2
  %t31 = load i64, ptr %j.6
  %t32 = add i64 %t30, %t31
  %t33 = getelementptr %struct.Point, ptr %t29, i64 %t32
  %t34 = load %struct.Point, ptr %t33
  store %struct.Point %t34, ptr %t28
  %t35 = load ptr, ptr %pts.1
  %t36 = load i64, ptr %left.2
  %t37 = load i64, ptr %j.6
  %t38 = add i64 %t36, %t37
  %t39 = getelementptr %struct.Point, ptr %t35, i64 %t38
  %t40 = load %struct.Point, ptr %tmp.7
  store %struct.Point %t40, ptr %t39
  %t41 = load ptr, ptr %pts.1
  %t42 = load i64, ptr %left.2
  %t43 = load i64, ptr %j.6
  call void @heap_sift_down_points(ptr %t41, i64 %t42, i64 0, i64 %t43)
  %t44 = load i64, ptr %j.6
  %t45 = sub i64 %t44, 1
  store i64 %t45, ptr %j.6
  br label %fpost9
fpost9:
  br label %fcond7
fend10:
  ret void
}

define void @introsort_points(ptr %arg.pts, i64 %arg.left, i64 %arg.right, i64 %arg.max_depth) alwaysinline {
entry:
  %pts.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %max_depth.4 = alloca i64
  %l.5 = alloca i64
  %r.6 = alloca i64
  %depth.7 = alloca i64
  %mid.8 = alloca i64
  %tmp.9 = alloca %struct.Point
  %tmp.10 = alloca %struct.Point
  %tmp.11 = alloca %struct.Point
  %piv_x.12 = alloca i64
  %piv_y.13 = alloca i64
  %i.14 = alloca i64
  %j.15 = alloca i64
  %tmp.16 = alloca %struct.Point
  store ptr %arg.pts, ptr %pts.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  store i64 %arg.max_depth, ptr %max_depth.4
  %t1 = load i64, ptr %left.2
  store i64 %t1, ptr %l.5
  %t2 = load i64, ptr %right.3
  store i64 %t2, ptr %r.6
  %t3 = load i64, ptr %max_depth.4
  store i64 %t3, ptr %depth.7
  br label %fcond1
fcond1:
  %t4 = load i64, ptr %r.6
  %t5 = load i64, ptr %l.5
  %t6 = sub i64 %t4, %t5
  %t7 = icmp sgt i64 %t6, 16
  br i1 %t7, label %fbody2, label %fend4
fbody2:
  %t8 = load i64, ptr %depth.7
  %t9 = icmp eq i64 %t8, 0
  br i1 %t9, label %then5, label %endif6
then5:
  %t10 = load ptr, ptr %pts.1
  %t11 = load i64, ptr %l.5
  %t12 = load i64, ptr %r.6
  call void @heapsort_points(ptr %t10, i64 %t11, i64 %t12)
  ret void
endif6:
  %t13 = load i64, ptr %depth.7
  %t14 = sub i64 %t13, 1
  store i64 %t14, ptr %depth.7
  %t15 = load i64, ptr %l.5
  %t16 = load i64, ptr %r.6
  %t17 = load i64, ptr %l.5
  %t18 = sub i64 %t16, %t17
  %t19 = sdiv i64 %t18, 2
  %t20 = add i64 %t15, %t19
  store i64 %t20, ptr %mid.8
  %t21 = load ptr, ptr %pts.1
  %t22 = load i64, ptr %mid.8
  %t23 = getelementptr %struct.Point, ptr %t21, i64 %t22
  %t24 = getelementptr %struct.Point, ptr %t23, i32 0, i32 0
  %t25 = load i64, ptr %t24
  %t26 = load ptr, ptr %pts.1
  %t27 = load i64, ptr %mid.8
  %t28 = getelementptr %struct.Point, ptr %t26, i64 %t27
  %t29 = getelementptr %struct.Point, ptr %t28, i32 0, i32 1
  %t30 = load i64, ptr %t29
  %t31 = load ptr, ptr %pts.1
  %t32 = load i64, ptr %l.5
  %t33 = getelementptr %struct.Point, ptr %t31, i64 %t32
  %t34 = getelementptr %struct.Point, ptr %t33, i32 0, i32 0
  %t35 = load i64, ptr %t34
  %t36 = load ptr, ptr %pts.1
  %t37 = load i64, ptr %l.5
  %t38 = getelementptr %struct.Point, ptr %t36, i64 %t37
  %t39 = getelementptr %struct.Point, ptr %t38, i32 0, i32 1
  %t40 = load i64, ptr %t39
  %t41 = call i1 @point_greater(i64 %t25, i64 %t30, i64 %t35, i64 %t40)
  br i1 %t41, label %then7, label %endif8
then7:
  %t42 = load ptr, ptr %pts.1
  %t43 = load i64, ptr %l.5
  %t44 = getelementptr %struct.Point, ptr %t42, i64 %t43
  %t45 = load %struct.Point, ptr %t44
  store %struct.Point %t45, ptr %tmp.9
  %t46 = load ptr, ptr %pts.1
  %t47 = load i64, ptr %l.5
  %t48 = getelementptr %struct.Point, ptr %t46, i64 %t47
  %t49 = load ptr, ptr %pts.1
  %t50 = load i64, ptr %mid.8
  %t51 = getelementptr %struct.Point, ptr %t49, i64 %t50
  %t52 = load %struct.Point, ptr %t51
  store %struct.Point %t52, ptr %t48
  %t53 = load ptr, ptr %pts.1
  %t54 = load i64, ptr %mid.8
  %t55 = getelementptr %struct.Point, ptr %t53, i64 %t54
  %t56 = load %struct.Point, ptr %tmp.9
  store %struct.Point %t56, ptr %t55
  br label %endif8
endif8:
  %t57 = load ptr, ptr %pts.1
  %t58 = load i64, ptr %r.6
  %t59 = getelementptr %struct.Point, ptr %t57, i64 %t58
  %t60 = getelementptr %struct.Point, ptr %t59, i32 0, i32 0
  %t61 = load i64, ptr %t60
  %t62 = load ptr, ptr %pts.1
  %t63 = load i64, ptr %r.6
  %t64 = getelementptr %struct.Point, ptr %t62, i64 %t63
  %t65 = getelementptr %struct.Point, ptr %t64, i32 0, i32 1
  %t66 = load i64, ptr %t65
  %t67 = load ptr, ptr %pts.1
  %t68 = load i64, ptr %l.5
  %t69 = getelementptr %struct.Point, ptr %t67, i64 %t68
  %t70 = getelementptr %struct.Point, ptr %t69, i32 0, i32 0
  %t71 = load i64, ptr %t70
  %t72 = load ptr, ptr %pts.1
  %t73 = load i64, ptr %l.5
  %t74 = getelementptr %struct.Point, ptr %t72, i64 %t73
  %t75 = getelementptr %struct.Point, ptr %t74, i32 0, i32 1
  %t76 = load i64, ptr %t75
  %t77 = call i1 @point_greater(i64 %t61, i64 %t66, i64 %t71, i64 %t76)
  br i1 %t77, label %then9, label %endif10
then9:
  %t78 = load ptr, ptr %pts.1
  %t79 = load i64, ptr %l.5
  %t80 = getelementptr %struct.Point, ptr %t78, i64 %t79
  %t81 = load %struct.Point, ptr %t80
  store %struct.Point %t81, ptr %tmp.10
  %t82 = load ptr, ptr %pts.1
  %t83 = load i64, ptr %l.5
  %t84 = getelementptr %struct.Point, ptr %t82, i64 %t83
  %t85 = load ptr, ptr %pts.1
  %t86 = load i64, ptr %r.6
  %t87 = getelementptr %struct.Point, ptr %t85, i64 %t86
  %t88 = load %struct.Point, ptr %t87
  store %struct.Point %t88, ptr %t84
  %t89 = load ptr, ptr %pts.1
  %t90 = load i64, ptr %r.6
  %t91 = getelementptr %struct.Point, ptr %t89, i64 %t90
  %t92 = load %struct.Point, ptr %tmp.10
  store %struct.Point %t92, ptr %t91
  br label %endif10
endif10:
  %t93 = load ptr, ptr %pts.1
  %t94 = load i64, ptr %r.6
  %t95 = getelementptr %struct.Point, ptr %t93, i64 %t94
  %t96 = getelementptr %struct.Point, ptr %t95, i32 0, i32 0
  %t97 = load i64, ptr %t96
  %t98 = load ptr, ptr %pts.1
  %t99 = load i64, ptr %r.6
  %t100 = getelementptr %struct.Point, ptr %t98, i64 %t99
  %t101 = getelementptr %struct.Point, ptr %t100, i32 0, i32 1
  %t102 = load i64, ptr %t101
  %t103 = load ptr, ptr %pts.1
  %t104 = load i64, ptr %mid.8
  %t105 = getelementptr %struct.Point, ptr %t103, i64 %t104
  %t106 = getelementptr %struct.Point, ptr %t105, i32 0, i32 0
  %t107 = load i64, ptr %t106
  %t108 = load ptr, ptr %pts.1
  %t109 = load i64, ptr %mid.8
  %t110 = getelementptr %struct.Point, ptr %t108, i64 %t109
  %t111 = getelementptr %struct.Point, ptr %t110, i32 0, i32 1
  %t112 = load i64, ptr %t111
  %t113 = call i1 @point_greater(i64 %t97, i64 %t102, i64 %t107, i64 %t112)
  br i1 %t113, label %then11, label %endif12
then11:
  %t114 = load ptr, ptr %pts.1
  %t115 = load i64, ptr %mid.8
  %t116 = getelementptr %struct.Point, ptr %t114, i64 %t115
  %t117 = load %struct.Point, ptr %t116
  store %struct.Point %t117, ptr %tmp.11
  %t118 = load ptr, ptr %pts.1
  %t119 = load i64, ptr %mid.8
  %t120 = getelementptr %struct.Point, ptr %t118, i64 %t119
  %t121 = load ptr, ptr %pts.1
  %t122 = load i64, ptr %r.6
  %t123 = getelementptr %struct.Point, ptr %t121, i64 %t122
  %t124 = load %struct.Point, ptr %t123
  store %struct.Point %t124, ptr %t120
  %t125 = load ptr, ptr %pts.1
  %t126 = load i64, ptr %r.6
  %t127 = getelementptr %struct.Point, ptr %t125, i64 %t126
  %t128 = load %struct.Point, ptr %tmp.11
  store %struct.Point %t128, ptr %t127
  br label %endif12
endif12:
  %t129 = load ptr, ptr %pts.1
  %t130 = load i64, ptr %mid.8
  %t131 = getelementptr %struct.Point, ptr %t129, i64 %t130
  %t132 = getelementptr %struct.Point, ptr %t131, i32 0, i32 0
  %t133 = load i64, ptr %t132
  store i64 %t133, ptr %piv_x.12
  %t134 = load ptr, ptr %pts.1
  %t135 = load i64, ptr %mid.8
  %t136 = getelementptr %struct.Point, ptr %t134, i64 %t135
  %t137 = getelementptr %struct.Point, ptr %t136, i32 0, i32 1
  %t138 = load i64, ptr %t137
  store i64 %t138, ptr %piv_y.13
  %t139 = load i64, ptr %l.5
  store i64 %t139, ptr %i.14
  %t140 = load i64, ptr %r.6
  store i64 %t140, ptr %j.15
  br label %fcond13
fcond13:
  %t141 = load i64, ptr %i.14
  %t142 = load i64, ptr %j.15
  %t143 = icmp sle i64 %t141, %t142
  br i1 %t143, label %fbody14, label %fend16
fbody14:
  br label %fcond17
fcond17:
  %t144 = load ptr, ptr %pts.1
  %t145 = load i64, ptr %i.14
  %t146 = getelementptr %struct.Point, ptr %t144, i64 %t145
  %t147 = getelementptr %struct.Point, ptr %t146, i32 0, i32 0
  %t148 = load i64, ptr %t147
  %t149 = load ptr, ptr %pts.1
  %t150 = load i64, ptr %i.14
  %t151 = getelementptr %struct.Point, ptr %t149, i64 %t150
  %t152 = getelementptr %struct.Point, ptr %t151, i32 0, i32 1
  %t153 = load i64, ptr %t152
  %t154 = load i64, ptr %piv_x.12
  %t155 = load i64, ptr %piv_y.13
  %t156 = call i1 @point_greater(i64 %t148, i64 %t153, i64 %t154, i64 %t155)
  br i1 %t156, label %fbody18, label %fend20
fbody18:
  %t157 = load i64, ptr %i.14
  %t158 = add i64 %t157, 1
  store i64 %t158, ptr %i.14
  br label %fpost19
fpost19:
  br label %fcond17
fend20:
  br label %fcond21
fcond21:
  %t159 = load i64, ptr %piv_x.12
  %t160 = load i64, ptr %piv_y.13
  %t161 = load ptr, ptr %pts.1
  %t162 = load i64, ptr %j.15
  %t163 = getelementptr %struct.Point, ptr %t161, i64 %t162
  %t164 = getelementptr %struct.Point, ptr %t163, i32 0, i32 0
  %t165 = load i64, ptr %t164
  %t166 = load ptr, ptr %pts.1
  %t167 = load i64, ptr %j.15
  %t168 = getelementptr %struct.Point, ptr %t166, i64 %t167
  %t169 = getelementptr %struct.Point, ptr %t168, i32 0, i32 1
  %t170 = load i64, ptr %t169
  %t171 = call i1 @point_greater(i64 %t159, i64 %t160, i64 %t165, i64 %t170)
  br i1 %t171, label %fbody22, label %fend24
fbody22:
  %t172 = load i64, ptr %j.15
  %t173 = sub i64 %t172, 1
  store i64 %t173, ptr %j.15
  br label %fpost23
fpost23:
  br label %fcond21
fend24:
  %t174 = load i64, ptr %i.14
  %t175 = load i64, ptr %j.15
  %t176 = icmp sle i64 %t174, %t175
  br i1 %t176, label %then25, label %endif26
then25:
  %t177 = load ptr, ptr %pts.1
  %t178 = load i64, ptr %i.14
  %t179 = getelementptr %struct.Point, ptr %t177, i64 %t178
  %t180 = load %struct.Point, ptr %t179
  store %struct.Point %t180, ptr %tmp.16
  %t181 = load ptr, ptr %pts.1
  %t182 = load i64, ptr %i.14
  %t183 = getelementptr %struct.Point, ptr %t181, i64 %t182
  %t184 = load ptr, ptr %pts.1
  %t185 = load i64, ptr %j.15
  %t186 = getelementptr %struct.Point, ptr %t184, i64 %t185
  %t187 = load %struct.Point, ptr %t186
  store %struct.Point %t187, ptr %t183
  %t188 = load ptr, ptr %pts.1
  %t189 = load i64, ptr %j.15
  %t190 = getelementptr %struct.Point, ptr %t188, i64 %t189
  %t191 = load %struct.Point, ptr %tmp.16
  store %struct.Point %t191, ptr %t190
  %t192 = load i64, ptr %i.14
  %t193 = add i64 %t192, 1
  store i64 %t193, ptr %i.14
  %t194 = load i64, ptr %j.15
  %t195 = sub i64 %t194, 1
  store i64 %t195, ptr %j.15
  br label %endif26
endif26:
  br label %fpost15
fpost15:
  br label %fcond13
fend16:
  %t196 = load i64, ptr %j.15
  %t197 = load i64, ptr %l.5
  %t198 = sub i64 %t196, %t197
  %t199 = load i64, ptr %r.6
  %t200 = load i64, ptr %i.14
  %t201 = sub i64 %t199, %t200
  %t202 = icmp slt i64 %t198, %t201
  br i1 %t202, label %then27, label %else29
then27:
  %t203 = load i64, ptr %l.5
  %t204 = load i64, ptr %j.15
  %t205 = icmp slt i64 %t203, %t204
  br i1 %t205, label %then30, label %endif31
then30:
  %t206 = load ptr, ptr %pts.1
  %t207 = load i64, ptr %l.5
  %t208 = load i64, ptr %j.15
  %t209 = load i64, ptr %depth.7
  call void @introsort_points(ptr %t206, i64 %t207, i64 %t208, i64 %t209)
  br label %endif31
endif31:
  %t210 = load i64, ptr %i.14
  store i64 %t210, ptr %l.5
  br label %endif28
else29:
  %t211 = load i64, ptr %i.14
  %t212 = load i64, ptr %r.6
  %t213 = icmp slt i64 %t211, %t212
  br i1 %t213, label %then32, label %endif33
then32:
  %t214 = load ptr, ptr %pts.1
  %t215 = load i64, ptr %i.14
  %t216 = load i64, ptr %r.6
  %t217 = load i64, ptr %depth.7
  call void @introsort_points(ptr %t214, i64 %t215, i64 %t216, i64 %t217)
  br label %endif33
endif33:
  %t218 = load i64, ptr %j.15
  store i64 %t218, ptr %r.6
  br label %endif28
endif28:
  br label %fpost3
fpost3:
  br label %fcond1
fend4:
  %t219 = load ptr, ptr %pts.1
  %t220 = load i64, ptr %l.5
  %t221 = load i64, ptr %r.6
  call void @insertion_sort_points(ptr %t219, i64 %t220, i64 %t221)
  ret void
}

define void @sort_points(ptr %arg.pts, i64 %arg.left, i64 %arg.right) alwaysinline {
entry:
  %pts.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %depth.4 = alloca i64
  store ptr %arg.pts, ptr %pts.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  %t1 = load i64, ptr %left.2
  %t2 = load i64, ptr %right.3
  %t3 = icmp sge i64 %t1, %t2
  br i1 %t3, label %then1, label %endif2
then1:
  ret void
endif2:
  %t4 = load i64, ptr %right.3
  %t5 = load i64, ptr %left.2
  %t6 = sub i64 %t4, %t5
  %t7 = add i64 %t6, 1
  %t8 = call i64 @calc_max_depth(i64 %t7)
  store i64 %t8, ptr %depth.4
  %t9 = load ptr, ptr %pts.1
  %t10 = load i64, ptr %left.2
  %t11 = load i64, ptr %right.3
  %t12 = load i64, ptr %depth.4
  call void @introsort_points(ptr %t9, i64 %t10, i64 %t11, i64 %t12)
  ret void
}

define void @insertion_sort_i64(ptr %arg.arr, i64 %arg.left, i64 %arg.right) alwaysinline {
entry:
  %arr.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %i.4 = alloca i64
  %key.5 = alloca i64
  %j.6 = alloca i64
  %logic.7 = alloca i1
  store ptr %arg.arr, ptr %arr.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  %t1 = load i64, ptr %left.2
  %t2 = add i64 %t1, 1
  store i64 %t2, ptr %i.4
  br label %fcond1
fcond1:
  %t3 = load i64, ptr %i.4
  %t4 = load i64, ptr %right.3
  %t5 = icmp sle i64 %t3, %t4
  br i1 %t5, label %fbody2, label %fend4
fbody2:
  %t6 = load ptr, ptr %arr.1
  %t7 = load i64, ptr %i.4
  %t8 = getelementptr i64, ptr %t6, i64 %t7
  %t9 = load i64, ptr %t8
  store i64 %t9, ptr %key.5
  %t10 = load i64, ptr %i.4
  %t11 = sub i64 %t10, 1
  store i64 %t11, ptr %j.6
  br label %fcond5
fcond5:
  %t12 = load i64, ptr %j.6
  %t13 = load i64, ptr %left.2
  %t14 = icmp sge i64 %t12, %t13
  store i1 %t14, ptr %logic.7
  br i1 %t14, label %logrhs9, label %logdone10
logrhs9:
  %t15 = load ptr, ptr %arr.1
  %t16 = load i64, ptr %j.6
  %t17 = getelementptr i64, ptr %t15, i64 %t16
  %t18 = load i64, ptr %t17
  %t19 = load i64, ptr %key.5
  %t20 = icmp sgt i64 %t18, %t19
  store i1 %t20, ptr %logic.7
  br label %logdone10
logdone10:
  %t21 = load i1, ptr %logic.7
  br i1 %t21, label %fbody6, label %fend8
fbody6:
  %t22 = load ptr, ptr %arr.1
  %t23 = load i64, ptr %j.6
  %t24 = add i64 %t23, 1
  %t25 = getelementptr i64, ptr %t22, i64 %t24
  %t26 = load ptr, ptr %arr.1
  %t27 = load i64, ptr %j.6
  %t28 = getelementptr i64, ptr %t26, i64 %t27
  %t29 = load i64, ptr %t28
  store i64 %t29, ptr %t25
  %t30 = load i64, ptr %j.6
  %t31 = sub i64 %t30, 1
  store i64 %t31, ptr %j.6
  br label %fpost7
fpost7:
  br label %fcond5
fend8:
  %t32 = load ptr, ptr %arr.1
  %t33 = load i64, ptr %j.6
  %t34 = add i64 %t33, 1
  %t35 = getelementptr i64, ptr %t32, i64 %t34
  %t36 = load i64, ptr %key.5
  store i64 %t36, ptr %t35
  br label %fpost3
fpost3:
  %t37 = load i64, ptr %i.4
  %t38 = add i64 %t37, 1
  store i64 %t38, ptr %i.4
  br label %fcond1
fend4:
  ret void
}

define void @heap_sift_down_i64(ptr %arg.arr, i64 %arg.left, i64 %arg.root, i64 %arg.n) alwaysinline {
entry:
  %arr.1 = alloca ptr
  %left.2 = alloca i64
  %root.3 = alloca i64
  %n.4 = alloca i64
  %curr.5 = alloca i64
  %largest.6 = alloca i64
  %left_child.7 = alloca i64
  %right_child.8 = alloca i64
  %logic.9 = alloca i1
  %logic.10 = alloca i1
  %tmp.11 = alloca i64
  store ptr %arg.arr, ptr %arr.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.root, ptr %root.3
  store i64 %arg.n, ptr %n.4
  %t1 = load i64, ptr %root.3
  store i64 %t1, ptr %curr.5
  br label %fcond1
fcond1:
  br label %fbody2
fbody2:
  %t2 = load i64, ptr %curr.5
  store i64 %t2, ptr %largest.6
  %t3 = load i64, ptr %curr.5
  %t4 = mul i64 2, %t3
  %t5 = add i64 %t4, 1
  store i64 %t5, ptr %left_child.7
  %t6 = load i64, ptr %curr.5
  %t7 = mul i64 2, %t6
  %t8 = add i64 %t7, 2
  store i64 %t8, ptr %right_child.8
  %t9 = load i64, ptr %left_child.7
  %t10 = load i64, ptr %n.4
  %t11 = icmp slt i64 %t9, %t10
  store i1 %t11, ptr %logic.9
  br i1 %t11, label %logrhs5, label %logdone6
logrhs5:
  %t12 = load ptr, ptr %arr.1
  %t13 = load i64, ptr %left.2
  %t14 = load i64, ptr %left_child.7
  %t15 = add i64 %t13, %t14
  %t16 = getelementptr i64, ptr %t12, i64 %t15
  %t17 = load i64, ptr %t16
  %t18 = load ptr, ptr %arr.1
  %t19 = load i64, ptr %left.2
  %t20 = load i64, ptr %largest.6
  %t21 = add i64 %t19, %t20
  %t22 = getelementptr i64, ptr %t18, i64 %t21
  %t23 = load i64, ptr %t22
  %t24 = icmp sgt i64 %t17, %t23
  store i1 %t24, ptr %logic.9
  br label %logdone6
logdone6:
  %t25 = load i1, ptr %logic.9
  br i1 %t25, label %then7, label %endif8
then7:
  %t26 = load i64, ptr %left_child.7
  store i64 %t26, ptr %largest.6
  br label %endif8
endif8:
  %t27 = load i64, ptr %right_child.8
  %t28 = load i64, ptr %n.4
  %t29 = icmp slt i64 %t27, %t28
  store i1 %t29, ptr %logic.10
  br i1 %t29, label %logrhs9, label %logdone10
logrhs9:
  %t30 = load ptr, ptr %arr.1
  %t31 = load i64, ptr %left.2
  %t32 = load i64, ptr %right_child.8
  %t33 = add i64 %t31, %t32
  %t34 = getelementptr i64, ptr %t30, i64 %t33
  %t35 = load i64, ptr %t34
  %t36 = load ptr, ptr %arr.1
  %t37 = load i64, ptr %left.2
  %t38 = load i64, ptr %largest.6
  %t39 = add i64 %t37, %t38
  %t40 = getelementptr i64, ptr %t36, i64 %t39
  %t41 = load i64, ptr %t40
  %t42 = icmp sgt i64 %t35, %t41
  store i1 %t42, ptr %logic.10
  br label %logdone10
logdone10:
  %t43 = load i1, ptr %logic.10
  br i1 %t43, label %then11, label %endif12
then11:
  %t44 = load i64, ptr %right_child.8
  store i64 %t44, ptr %largest.6
  br label %endif12
endif12:
  %t45 = load i64, ptr %largest.6
  %t46 = load i64, ptr %curr.5
  %t47 = icmp eq i64 %t45, %t46
  br i1 %t47, label %then13, label %endif14
then13:
  br label %fend4
endif14:
  %t48 = load ptr, ptr %arr.1
  %t49 = load i64, ptr %left.2
  %t50 = load i64, ptr %curr.5
  %t51 = add i64 %t49, %t50
  %t52 = getelementptr i64, ptr %t48, i64 %t51
  %t53 = load i64, ptr %t52
  store i64 %t53, ptr %tmp.11
  %t54 = load ptr, ptr %arr.1
  %t55 = load i64, ptr %left.2
  %t56 = load i64, ptr %curr.5
  %t57 = add i64 %t55, %t56
  %t58 = getelementptr i64, ptr %t54, i64 %t57
  %t59 = load ptr, ptr %arr.1
  %t60 = load i64, ptr %left.2
  %t61 = load i64, ptr %largest.6
  %t62 = add i64 %t60, %t61
  %t63 = getelementptr i64, ptr %t59, i64 %t62
  %t64 = load i64, ptr %t63
  store i64 %t64, ptr %t58
  %t65 = load ptr, ptr %arr.1
  %t66 = load i64, ptr %left.2
  %t67 = load i64, ptr %largest.6
  %t68 = add i64 %t66, %t67
  %t69 = getelementptr i64, ptr %t65, i64 %t68
  %t70 = load i64, ptr %tmp.11
  store i64 %t70, ptr %t69
  %t71 = load i64, ptr %largest.6
  store i64 %t71, ptr %curr.5
  br label %fpost3
fpost3:
  br label %fcond1
fend4:
  ret void
}

define void @heapsort_i64(ptr %arg.arr, i64 %arg.left, i64 %arg.right) alwaysinline {
entry:
  %arr.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %n.4 = alloca i64
  %i.5 = alloca i64
  %j.6 = alloca i64
  %tmp.7 = alloca i64
  store ptr %arg.arr, ptr %arr.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  %t1 = load i64, ptr %right.3
  %t2 = load i64, ptr %left.2
  %t3 = sub i64 %t1, %t2
  %t4 = add i64 %t3, 1
  store i64 %t4, ptr %n.4
  %t5 = load i64, ptr %n.4
  %t6 = icmp sle i64 %t5, 1
  br i1 %t6, label %then1, label %endif2
then1:
  ret void
endif2:
  %t7 = load i64, ptr %n.4
  %t8 = sdiv i64 %t7, 2
  %t9 = sub i64 %t8, 1
  store i64 %t9, ptr %i.5
  br label %fcond3
fcond3:
  %t10 = load i64, ptr %i.5
  %t11 = icmp sge i64 %t10, 0
  br i1 %t11, label %fbody4, label %fend6
fbody4:
  %t12 = load ptr, ptr %arr.1
  %t13 = load i64, ptr %left.2
  %t14 = load i64, ptr %i.5
  %t15 = load i64, ptr %n.4
  call void @heap_sift_down_i64(ptr %t12, i64 %t13, i64 %t14, i64 %t15)
  %t16 = load i64, ptr %i.5
  %t17 = sub i64 %t16, 1
  store i64 %t17, ptr %i.5
  br label %fpost5
fpost5:
  br label %fcond3
fend6:
  %t18 = load i64, ptr %n.4
  %t19 = sub i64 %t18, 1
  store i64 %t19, ptr %j.6
  br label %fcond7
fcond7:
  %t20 = load i64, ptr %j.6
  %t21 = icmp sgt i64 %t20, 0
  br i1 %t21, label %fbody8, label %fend10
fbody8:
  %t22 = load ptr, ptr %arr.1
  %t23 = load i64, ptr %left.2
  %t24 = getelementptr i64, ptr %t22, i64 %t23
  %t25 = load i64, ptr %t24
  store i64 %t25, ptr %tmp.7
  %t26 = load ptr, ptr %arr.1
  %t27 = load i64, ptr %left.2
  %t28 = getelementptr i64, ptr %t26, i64 %t27
  %t29 = load ptr, ptr %arr.1
  %t30 = load i64, ptr %left.2
  %t31 = load i64, ptr %j.6
  %t32 = add i64 %t30, %t31
  %t33 = getelementptr i64, ptr %t29, i64 %t32
  %t34 = load i64, ptr %t33
  store i64 %t34, ptr %t28
  %t35 = load ptr, ptr %arr.1
  %t36 = load i64, ptr %left.2
  %t37 = load i64, ptr %j.6
  %t38 = add i64 %t36, %t37
  %t39 = getelementptr i64, ptr %t35, i64 %t38
  %t40 = load i64, ptr %tmp.7
  store i64 %t40, ptr %t39
  %t41 = load ptr, ptr %arr.1
  %t42 = load i64, ptr %left.2
  %t43 = load i64, ptr %j.6
  call void @heap_sift_down_i64(ptr %t41, i64 %t42, i64 0, i64 %t43)
  %t44 = load i64, ptr %j.6
  %t45 = sub i64 %t44, 1
  store i64 %t45, ptr %j.6
  br label %fpost9
fpost9:
  br label %fcond7
fend10:
  ret void
}

define void @introsort_i64(ptr %arg.arr, i64 %arg.left, i64 %arg.right, i64 %arg.max_depth) alwaysinline {
entry:
  %arr.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %max_depth.4 = alloca i64
  %l.5 = alloca i64
  %r.6 = alloca i64
  %depth.7 = alloca i64
  %mid.8 = alloca i64
  %tmp.9 = alloca i64
  %tmp.10 = alloca i64
  %tmp.11 = alloca i64
  %pivot.12 = alloca i64
  %i.13 = alloca i64
  %j.14 = alloca i64
  %tmp.15 = alloca i64
  store ptr %arg.arr, ptr %arr.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  store i64 %arg.max_depth, ptr %max_depth.4
  %t1 = load i64, ptr %left.2
  store i64 %t1, ptr %l.5
  %t2 = load i64, ptr %right.3
  store i64 %t2, ptr %r.6
  %t3 = load i64, ptr %max_depth.4
  store i64 %t3, ptr %depth.7
  br label %fcond1
fcond1:
  %t4 = load i64, ptr %r.6
  %t5 = load i64, ptr %l.5
  %t6 = sub i64 %t4, %t5
  %t7 = icmp sgt i64 %t6, 16
  br i1 %t7, label %fbody2, label %fend4
fbody2:
  %t8 = load i64, ptr %depth.7
  %t9 = icmp eq i64 %t8, 0
  br i1 %t9, label %then5, label %endif6
then5:
  %t10 = load ptr, ptr %arr.1
  %t11 = load i64, ptr %l.5
  %t12 = load i64, ptr %r.6
  call void @heapsort_i64(ptr %t10, i64 %t11, i64 %t12)
  ret void
endif6:
  %t13 = load i64, ptr %depth.7
  %t14 = sub i64 %t13, 1
  store i64 %t14, ptr %depth.7
  %t15 = load i64, ptr %l.5
  %t16 = load i64, ptr %r.6
  %t17 = load i64, ptr %l.5
  %t18 = sub i64 %t16, %t17
  %t19 = sdiv i64 %t18, 2
  %t20 = add i64 %t15, %t19
  store i64 %t20, ptr %mid.8
  %t21 = load ptr, ptr %arr.1
  %t22 = load i64, ptr %l.5
  %t23 = getelementptr i64, ptr %t21, i64 %t22
  %t24 = load i64, ptr %t23
  %t25 = load ptr, ptr %arr.1
  %t26 = load i64, ptr %mid.8
  %t27 = getelementptr i64, ptr %t25, i64 %t26
  %t28 = load i64, ptr %t27
  %t29 = icmp sgt i64 %t24, %t28
  br i1 %t29, label %then7, label %endif8
then7:
  %t30 = load ptr, ptr %arr.1
  %t31 = load i64, ptr %l.5
  %t32 = getelementptr i64, ptr %t30, i64 %t31
  %t33 = load i64, ptr %t32
  store i64 %t33, ptr %tmp.9
  %t34 = load ptr, ptr %arr.1
  %t35 = load i64, ptr %l.5
  %t36 = getelementptr i64, ptr %t34, i64 %t35
  %t37 = load ptr, ptr %arr.1
  %t38 = load i64, ptr %mid.8
  %t39 = getelementptr i64, ptr %t37, i64 %t38
  %t40 = load i64, ptr %t39
  store i64 %t40, ptr %t36
  %t41 = load ptr, ptr %arr.1
  %t42 = load i64, ptr %mid.8
  %t43 = getelementptr i64, ptr %t41, i64 %t42
  %t44 = load i64, ptr %tmp.9
  store i64 %t44, ptr %t43
  br label %endif8
endif8:
  %t45 = load ptr, ptr %arr.1
  %t46 = load i64, ptr %l.5
  %t47 = getelementptr i64, ptr %t45, i64 %t46
  %t48 = load i64, ptr %t47
  %t49 = load ptr, ptr %arr.1
  %t50 = load i64, ptr %r.6
  %t51 = getelementptr i64, ptr %t49, i64 %t50
  %t52 = load i64, ptr %t51
  %t53 = icmp sgt i64 %t48, %t52
  br i1 %t53, label %then9, label %endif10
then9:
  %t54 = load ptr, ptr %arr.1
  %t55 = load i64, ptr %l.5
  %t56 = getelementptr i64, ptr %t54, i64 %t55
  %t57 = load i64, ptr %t56
  store i64 %t57, ptr %tmp.10
  %t58 = load ptr, ptr %arr.1
  %t59 = load i64, ptr %l.5
  %t60 = getelementptr i64, ptr %t58, i64 %t59
  %t61 = load ptr, ptr %arr.1
  %t62 = load i64, ptr %r.6
  %t63 = getelementptr i64, ptr %t61, i64 %t62
  %t64 = load i64, ptr %t63
  store i64 %t64, ptr %t60
  %t65 = load ptr, ptr %arr.1
  %t66 = load i64, ptr %r.6
  %t67 = getelementptr i64, ptr %t65, i64 %t66
  %t68 = load i64, ptr %tmp.10
  store i64 %t68, ptr %t67
  br label %endif10
endif10:
  %t69 = load ptr, ptr %arr.1
  %t70 = load i64, ptr %mid.8
  %t71 = getelementptr i64, ptr %t69, i64 %t70
  %t72 = load i64, ptr %t71
  %t73 = load ptr, ptr %arr.1
  %t74 = load i64, ptr %r.6
  %t75 = getelementptr i64, ptr %t73, i64 %t74
  %t76 = load i64, ptr %t75
  %t77 = icmp sgt i64 %t72, %t76
  br i1 %t77, label %then11, label %endif12
then11:
  %t78 = load ptr, ptr %arr.1
  %t79 = load i64, ptr %mid.8
  %t80 = getelementptr i64, ptr %t78, i64 %t79
  %t81 = load i64, ptr %t80
  store i64 %t81, ptr %tmp.11
  %t82 = load ptr, ptr %arr.1
  %t83 = load i64, ptr %mid.8
  %t84 = getelementptr i64, ptr %t82, i64 %t83
  %t85 = load ptr, ptr %arr.1
  %t86 = load i64, ptr %r.6
  %t87 = getelementptr i64, ptr %t85, i64 %t86
  %t88 = load i64, ptr %t87
  store i64 %t88, ptr %t84
  %t89 = load ptr, ptr %arr.1
  %t90 = load i64, ptr %r.6
  %t91 = getelementptr i64, ptr %t89, i64 %t90
  %t92 = load i64, ptr %tmp.11
  store i64 %t92, ptr %t91
  br label %endif12
endif12:
  %t93 = load ptr, ptr %arr.1
  %t94 = load i64, ptr %mid.8
  %t95 = getelementptr i64, ptr %t93, i64 %t94
  %t96 = load i64, ptr %t95
  store i64 %t96, ptr %pivot.12
  %t97 = load i64, ptr %l.5
  store i64 %t97, ptr %i.13
  %t98 = load i64, ptr %r.6
  store i64 %t98, ptr %j.14
  br label %fcond13
fcond13:
  %t99 = load i64, ptr %i.13
  %t100 = load i64, ptr %j.14
  %t101 = icmp sle i64 %t99, %t100
  br i1 %t101, label %fbody14, label %fend16
fbody14:
  br label %fcond17
fcond17:
  %t102 = load ptr, ptr %arr.1
  %t103 = load i64, ptr %i.13
  %t104 = getelementptr i64, ptr %t102, i64 %t103
  %t105 = load i64, ptr %t104
  %t106 = load i64, ptr %pivot.12
  %t107 = icmp slt i64 %t105, %t106
  br i1 %t107, label %fbody18, label %fend20
fbody18:
  %t108 = load i64, ptr %i.13
  %t109 = add i64 %t108, 1
  store i64 %t109, ptr %i.13
  br label %fpost19
fpost19:
  br label %fcond17
fend20:
  br label %fcond21
fcond21:
  %t110 = load ptr, ptr %arr.1
  %t111 = load i64, ptr %j.14
  %t112 = getelementptr i64, ptr %t110, i64 %t111
  %t113 = load i64, ptr %t112
  %t114 = load i64, ptr %pivot.12
  %t115 = icmp sgt i64 %t113, %t114
  br i1 %t115, label %fbody22, label %fend24
fbody22:
  %t116 = load i64, ptr %j.14
  %t117 = sub i64 %t116, 1
  store i64 %t117, ptr %j.14
  br label %fpost23
fpost23:
  br label %fcond21
fend24:
  %t118 = load i64, ptr %i.13
  %t119 = load i64, ptr %j.14
  %t120 = icmp sle i64 %t118, %t119
  br i1 %t120, label %then25, label %endif26
then25:
  %t121 = load ptr, ptr %arr.1
  %t122 = load i64, ptr %i.13
  %t123 = getelementptr i64, ptr %t121, i64 %t122
  %t124 = load i64, ptr %t123
  store i64 %t124, ptr %tmp.15
  %t125 = load ptr, ptr %arr.1
  %t126 = load i64, ptr %i.13
  %t127 = getelementptr i64, ptr %t125, i64 %t126
  %t128 = load ptr, ptr %arr.1
  %t129 = load i64, ptr %j.14
  %t130 = getelementptr i64, ptr %t128, i64 %t129
  %t131 = load i64, ptr %t130
  store i64 %t131, ptr %t127
  %t132 = load ptr, ptr %arr.1
  %t133 = load i64, ptr %j.14
  %t134 = getelementptr i64, ptr %t132, i64 %t133
  %t135 = load i64, ptr %tmp.15
  store i64 %t135, ptr %t134
  %t136 = load i64, ptr %i.13
  %t137 = add i64 %t136, 1
  store i64 %t137, ptr %i.13
  %t138 = load i64, ptr %j.14
  %t139 = sub i64 %t138, 1
  store i64 %t139, ptr %j.14
  br label %endif26
endif26:
  br label %fpost15
fpost15:
  br label %fcond13
fend16:
  %t140 = load i64, ptr %j.14
  %t141 = load i64, ptr %l.5
  %t142 = sub i64 %t140, %t141
  %t143 = load i64, ptr %r.6
  %t144 = load i64, ptr %i.13
  %t145 = sub i64 %t143, %t144
  %t146 = icmp slt i64 %t142, %t145
  br i1 %t146, label %then27, label %else29
then27:
  %t147 = load i64, ptr %l.5
  %t148 = load i64, ptr %j.14
  %t149 = icmp slt i64 %t147, %t148
  br i1 %t149, label %then30, label %endif31
then30:
  %t150 = load ptr, ptr %arr.1
  %t151 = load i64, ptr %l.5
  %t152 = load i64, ptr %j.14
  %t153 = load i64, ptr %depth.7
  call void @introsort_i64(ptr %t150, i64 %t151, i64 %t152, i64 %t153)
  br label %endif31
endif31:
  %t154 = load i64, ptr %i.13
  store i64 %t154, ptr %l.5
  br label %endif28
else29:
  %t155 = load i64, ptr %i.13
  %t156 = load i64, ptr %r.6
  %t157 = icmp slt i64 %t155, %t156
  br i1 %t157, label %then32, label %endif33
then32:
  %t158 = load ptr, ptr %arr.1
  %t159 = load i64, ptr %i.13
  %t160 = load i64, ptr %r.6
  %t161 = load i64, ptr %depth.7
  call void @introsort_i64(ptr %t158, i64 %t159, i64 %t160, i64 %t161)
  br label %endif33
endif33:
  %t162 = load i64, ptr %j.14
  store i64 %t162, ptr %r.6
  br label %endif28
endif28:
  br label %fpost3
fpost3:
  br label %fcond1
fend4:
  %t163 = load ptr, ptr %arr.1
  %t164 = load i64, ptr %l.5
  %t165 = load i64, ptr %r.6
  call void @insertion_sort_i64(ptr %t163, i64 %t164, i64 %t165)
  ret void
}

define void @sort_i64(ptr %arg.arr, i64 %arg.left, i64 %arg.right) alwaysinline {
entry:
  %arr.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %depth.4 = alloca i64
  store ptr %arg.arr, ptr %arr.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  %t1 = load i64, ptr %left.2
  %t2 = load i64, ptr %right.3
  %t3 = icmp sge i64 %t1, %t2
  br i1 %t3, label %then1, label %endif2
then1:
  ret void
endif2:
  %t4 = load i64, ptr %right.3
  %t5 = load i64, ptr %left.2
  %t6 = sub i64 %t4, %t5
  %t7 = add i64 %t6, 1
  %t8 = call i64 @calc_max_depth(i64 %t7)
  store i64 %t8, ptr %depth.4
  %t9 = load ptr, ptr %arr.1
  %t10 = load i64, ptr %left.2
  %t11 = load i64, ptr %right.3
  %t12 = load i64, ptr %depth.4
  call void @introsort_i64(ptr %t9, i64 %t10, i64 %t11, i64 %t12)
  ret void
}

define void @insertion_sort_edges(ptr %arg.edges, i64 %arg.left, i64 %arg.right) alwaysinline {
entry:
  %edges.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %i.4 = alloca i64
  %key.5 = alloca %struct.Edge
  %j.6 = alloca i64
  %logic.7 = alloca i1
  store ptr %arg.edges, ptr %edges.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  %t1 = load i64, ptr %left.2
  %t2 = add i64 %t1, 1
  store i64 %t2, ptr %i.4
  br label %fcond1
fcond1:
  %t3 = load i64, ptr %i.4
  %t4 = load i64, ptr %right.3
  %t5 = icmp sle i64 %t3, %t4
  br i1 %t5, label %fbody2, label %fend4
fbody2:
  %t6 = load ptr, ptr %edges.1
  %t7 = load i64, ptr %i.4
  %t8 = getelementptr %struct.Edge, ptr %t6, i64 %t7
  %t9 = load %struct.Edge, ptr %t8
  store %struct.Edge %t9, ptr %key.5
  %t10 = load i64, ptr %i.4
  %t11 = sub i64 %t10, 1
  store i64 %t11, ptr %j.6
  br label %fcond5
fcond5:
  %t12 = load i64, ptr %j.6
  %t13 = load i64, ptr %left.2
  %t14 = icmp sge i64 %t12, %t13
  store i1 %t14, ptr %logic.7
  br i1 %t14, label %logrhs9, label %logdone10
logrhs9:
  %t15 = load ptr, ptr %edges.1
  %t16 = load i64, ptr %j.6
  %t17 = getelementptr %struct.Edge, ptr %t15, i64 %t16
  %t18 = getelementptr %struct.Edge, ptr %t17, i32 0, i32 2
  %t19 = load i64, ptr %t18
  %t20 = getelementptr %struct.Edge, ptr %key.5, i32 0, i32 2
  %t21 = load i64, ptr %t20
  %t22 = icmp sgt i64 %t19, %t21
  store i1 %t22, ptr %logic.7
  br label %logdone10
logdone10:
  %t23 = load i1, ptr %logic.7
  br i1 %t23, label %fbody6, label %fend8
fbody6:
  %t24 = load ptr, ptr %edges.1
  %t25 = load i64, ptr %j.6
  %t26 = add i64 %t25, 1
  %t27 = getelementptr %struct.Edge, ptr %t24, i64 %t26
  %t28 = load ptr, ptr %edges.1
  %t29 = load i64, ptr %j.6
  %t30 = getelementptr %struct.Edge, ptr %t28, i64 %t29
  %t31 = load %struct.Edge, ptr %t30
  store %struct.Edge %t31, ptr %t27
  %t32 = load i64, ptr %j.6
  %t33 = sub i64 %t32, 1
  store i64 %t33, ptr %j.6
  br label %fpost7
fpost7:
  br label %fcond5
fend8:
  %t34 = load ptr, ptr %edges.1
  %t35 = load i64, ptr %j.6
  %t36 = add i64 %t35, 1
  %t37 = getelementptr %struct.Edge, ptr %t34, i64 %t36
  %t38 = load %struct.Edge, ptr %key.5
  store %struct.Edge %t38, ptr %t37
  br label %fpost3
fpost3:
  %t39 = load i64, ptr %i.4
  %t40 = add i64 %t39, 1
  store i64 %t40, ptr %i.4
  br label %fcond1
fend4:
  ret void
}

define void @heap_sift_down_edges(ptr %arg.edges, i64 %arg.left, i64 %arg.root, i64 %arg.n) alwaysinline {
entry:
  %edges.1 = alloca ptr
  %left.2 = alloca i64
  %root.3 = alloca i64
  %n.4 = alloca i64
  %curr.5 = alloca i64
  %largest.6 = alloca i64
  %left_child.7 = alloca i64
  %right_child.8 = alloca i64
  %logic.9 = alloca i1
  %logic.10 = alloca i1
  %tmp.11 = alloca %struct.Edge
  store ptr %arg.edges, ptr %edges.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.root, ptr %root.3
  store i64 %arg.n, ptr %n.4
  %t1 = load i64, ptr %root.3
  store i64 %t1, ptr %curr.5
  br label %fcond1
fcond1:
  br label %fbody2
fbody2:
  %t2 = load i64, ptr %curr.5
  store i64 %t2, ptr %largest.6
  %t3 = load i64, ptr %curr.5
  %t4 = mul i64 2, %t3
  %t5 = add i64 %t4, 1
  store i64 %t5, ptr %left_child.7
  %t6 = load i64, ptr %curr.5
  %t7 = mul i64 2, %t6
  %t8 = add i64 %t7, 2
  store i64 %t8, ptr %right_child.8
  %t9 = load i64, ptr %left_child.7
  %t10 = load i64, ptr %n.4
  %t11 = icmp slt i64 %t9, %t10
  store i1 %t11, ptr %logic.9
  br i1 %t11, label %logrhs5, label %logdone6
logrhs5:
  %t12 = load ptr, ptr %edges.1
  %t13 = load i64, ptr %left.2
  %t14 = load i64, ptr %left_child.7
  %t15 = add i64 %t13, %t14
  %t16 = getelementptr %struct.Edge, ptr %t12, i64 %t15
  %t17 = getelementptr %struct.Edge, ptr %t16, i32 0, i32 2
  %t18 = load i64, ptr %t17
  %t19 = load ptr, ptr %edges.1
  %t20 = load i64, ptr %left.2
  %t21 = load i64, ptr %largest.6
  %t22 = add i64 %t20, %t21
  %t23 = getelementptr %struct.Edge, ptr %t19, i64 %t22
  %t24 = getelementptr %struct.Edge, ptr %t23, i32 0, i32 2
  %t25 = load i64, ptr %t24
  %t26 = icmp sgt i64 %t18, %t25
  store i1 %t26, ptr %logic.9
  br label %logdone6
logdone6:
  %t27 = load i1, ptr %logic.9
  br i1 %t27, label %then7, label %endif8
then7:
  %t28 = load i64, ptr %left_child.7
  store i64 %t28, ptr %largest.6
  br label %endif8
endif8:
  %t29 = load i64, ptr %right_child.8
  %t30 = load i64, ptr %n.4
  %t31 = icmp slt i64 %t29, %t30
  store i1 %t31, ptr %logic.10
  br i1 %t31, label %logrhs9, label %logdone10
logrhs9:
  %t32 = load ptr, ptr %edges.1
  %t33 = load i64, ptr %left.2
  %t34 = load i64, ptr %right_child.8
  %t35 = add i64 %t33, %t34
  %t36 = getelementptr %struct.Edge, ptr %t32, i64 %t35
  %t37 = getelementptr %struct.Edge, ptr %t36, i32 0, i32 2
  %t38 = load i64, ptr %t37
  %t39 = load ptr, ptr %edges.1
  %t40 = load i64, ptr %left.2
  %t41 = load i64, ptr %largest.6
  %t42 = add i64 %t40, %t41
  %t43 = getelementptr %struct.Edge, ptr %t39, i64 %t42
  %t44 = getelementptr %struct.Edge, ptr %t43, i32 0, i32 2
  %t45 = load i64, ptr %t44
  %t46 = icmp sgt i64 %t38, %t45
  store i1 %t46, ptr %logic.10
  br label %logdone10
logdone10:
  %t47 = load i1, ptr %logic.10
  br i1 %t47, label %then11, label %endif12
then11:
  %t48 = load i64, ptr %right_child.8
  store i64 %t48, ptr %largest.6
  br label %endif12
endif12:
  %t49 = load i64, ptr %largest.6
  %t50 = load i64, ptr %curr.5
  %t51 = icmp eq i64 %t49, %t50
  br i1 %t51, label %then13, label %endif14
then13:
  br label %fend4
endif14:
  %t52 = load ptr, ptr %edges.1
  %t53 = load i64, ptr %left.2
  %t54 = load i64, ptr %curr.5
  %t55 = add i64 %t53, %t54
  %t56 = getelementptr %struct.Edge, ptr %t52, i64 %t55
  %t57 = load %struct.Edge, ptr %t56
  store %struct.Edge %t57, ptr %tmp.11
  %t58 = load ptr, ptr %edges.1
  %t59 = load i64, ptr %left.2
  %t60 = load i64, ptr %curr.5
  %t61 = add i64 %t59, %t60
  %t62 = getelementptr %struct.Edge, ptr %t58, i64 %t61
  %t63 = load ptr, ptr %edges.1
  %t64 = load i64, ptr %left.2
  %t65 = load i64, ptr %largest.6
  %t66 = add i64 %t64, %t65
  %t67 = getelementptr %struct.Edge, ptr %t63, i64 %t66
  %t68 = load %struct.Edge, ptr %t67
  store %struct.Edge %t68, ptr %t62
  %t69 = load ptr, ptr %edges.1
  %t70 = load i64, ptr %left.2
  %t71 = load i64, ptr %largest.6
  %t72 = add i64 %t70, %t71
  %t73 = getelementptr %struct.Edge, ptr %t69, i64 %t72
  %t74 = load %struct.Edge, ptr %tmp.11
  store %struct.Edge %t74, ptr %t73
  %t75 = load i64, ptr %largest.6
  store i64 %t75, ptr %curr.5
  br label %fpost3
fpost3:
  br label %fcond1
fend4:
  ret void
}

define void @heapsort_edges(ptr %arg.edges, i64 %arg.left, i64 %arg.right) alwaysinline {
entry:
  %edges.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %n.4 = alloca i64
  %i.5 = alloca i64
  %j.6 = alloca i64
  %tmp.7 = alloca %struct.Edge
  store ptr %arg.edges, ptr %edges.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  %t1 = load i64, ptr %right.3
  %t2 = load i64, ptr %left.2
  %t3 = sub i64 %t1, %t2
  %t4 = add i64 %t3, 1
  store i64 %t4, ptr %n.4
  %t5 = load i64, ptr %n.4
  %t6 = icmp sle i64 %t5, 1
  br i1 %t6, label %then1, label %endif2
then1:
  ret void
endif2:
  %t7 = load i64, ptr %n.4
  %t8 = sdiv i64 %t7, 2
  %t9 = sub i64 %t8, 1
  store i64 %t9, ptr %i.5
  br label %fcond3
fcond3:
  %t10 = load i64, ptr %i.5
  %t11 = icmp sge i64 %t10, 0
  br i1 %t11, label %fbody4, label %fend6
fbody4:
  %t12 = load ptr, ptr %edges.1
  %t13 = load i64, ptr %left.2
  %t14 = load i64, ptr %i.5
  %t15 = load i64, ptr %n.4
  call void @heap_sift_down_edges(ptr %t12, i64 %t13, i64 %t14, i64 %t15)
  %t16 = load i64, ptr %i.5
  %t17 = sub i64 %t16, 1
  store i64 %t17, ptr %i.5
  br label %fpost5
fpost5:
  br label %fcond3
fend6:
  %t18 = load i64, ptr %n.4
  %t19 = sub i64 %t18, 1
  store i64 %t19, ptr %j.6
  br label %fcond7
fcond7:
  %t20 = load i64, ptr %j.6
  %t21 = icmp sgt i64 %t20, 0
  br i1 %t21, label %fbody8, label %fend10
fbody8:
  %t22 = load ptr, ptr %edges.1
  %t23 = load i64, ptr %left.2
  %t24 = getelementptr %struct.Edge, ptr %t22, i64 %t23
  %t25 = load %struct.Edge, ptr %t24
  store %struct.Edge %t25, ptr %tmp.7
  %t26 = load ptr, ptr %edges.1
  %t27 = load i64, ptr %left.2
  %t28 = getelementptr %struct.Edge, ptr %t26, i64 %t27
  %t29 = load ptr, ptr %edges.1
  %t30 = load i64, ptr %left.2
  %t31 = load i64, ptr %j.6
  %t32 = add i64 %t30, %t31
  %t33 = getelementptr %struct.Edge, ptr %t29, i64 %t32
  %t34 = load %struct.Edge, ptr %t33
  store %struct.Edge %t34, ptr %t28
  %t35 = load ptr, ptr %edges.1
  %t36 = load i64, ptr %left.2
  %t37 = load i64, ptr %j.6
  %t38 = add i64 %t36, %t37
  %t39 = getelementptr %struct.Edge, ptr %t35, i64 %t38
  %t40 = load %struct.Edge, ptr %tmp.7
  store %struct.Edge %t40, ptr %t39
  %t41 = load ptr, ptr %edges.1
  %t42 = load i64, ptr %left.2
  %t43 = load i64, ptr %j.6
  call void @heap_sift_down_edges(ptr %t41, i64 %t42, i64 0, i64 %t43)
  %t44 = load i64, ptr %j.6
  %t45 = sub i64 %t44, 1
  store i64 %t45, ptr %j.6
  br label %fpost9
fpost9:
  br label %fcond7
fend10:
  ret void
}

define void @introsort_edges(ptr %arg.edges, i64 %arg.left, i64 %arg.right, i64 %arg.max_depth) alwaysinline {
entry:
  %edges.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %max_depth.4 = alloca i64
  %l.5 = alloca i64
  %r.6 = alloca i64
  %depth.7 = alloca i64
  %mid.8 = alloca i64
  %tmp.9 = alloca %struct.Edge
  %tmp.10 = alloca %struct.Edge
  %tmp.11 = alloca %struct.Edge
  %piv_w.12 = alloca i64
  %i.13 = alloca i64
  %j.14 = alloca i64
  %tmp.15 = alloca %struct.Edge
  store ptr %arg.edges, ptr %edges.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  store i64 %arg.max_depth, ptr %max_depth.4
  %t1 = load i64, ptr %left.2
  store i64 %t1, ptr %l.5
  %t2 = load i64, ptr %right.3
  store i64 %t2, ptr %r.6
  %t3 = load i64, ptr %max_depth.4
  store i64 %t3, ptr %depth.7
  br label %fcond1
fcond1:
  %t4 = load i64, ptr %r.6
  %t5 = load i64, ptr %l.5
  %t6 = sub i64 %t4, %t5
  %t7 = icmp sgt i64 %t6, 16
  br i1 %t7, label %fbody2, label %fend4
fbody2:
  %t8 = load i64, ptr %depth.7
  %t9 = icmp eq i64 %t8, 0
  br i1 %t9, label %then5, label %endif6
then5:
  %t10 = load ptr, ptr %edges.1
  %t11 = load i64, ptr %l.5
  %t12 = load i64, ptr %r.6
  call void @heapsort_edges(ptr %t10, i64 %t11, i64 %t12)
  ret void
endif6:
  %t13 = load i64, ptr %depth.7
  %t14 = sub i64 %t13, 1
  store i64 %t14, ptr %depth.7
  %t15 = load i64, ptr %l.5
  %t16 = load i64, ptr %r.6
  %t17 = load i64, ptr %l.5
  %t18 = sub i64 %t16, %t17
  %t19 = sdiv i64 %t18, 2
  %t20 = add i64 %t15, %t19
  store i64 %t20, ptr %mid.8
  %t21 = load ptr, ptr %edges.1
  %t22 = load i64, ptr %l.5
  %t23 = getelementptr %struct.Edge, ptr %t21, i64 %t22
  %t24 = getelementptr %struct.Edge, ptr %t23, i32 0, i32 2
  %t25 = load i64, ptr %t24
  %t26 = load ptr, ptr %edges.1
  %t27 = load i64, ptr %mid.8
  %t28 = getelementptr %struct.Edge, ptr %t26, i64 %t27
  %t29 = getelementptr %struct.Edge, ptr %t28, i32 0, i32 2
  %t30 = load i64, ptr %t29
  %t31 = icmp sgt i64 %t25, %t30
  br i1 %t31, label %then7, label %endif8
then7:
  %t32 = load ptr, ptr %edges.1
  %t33 = load i64, ptr %l.5
  %t34 = getelementptr %struct.Edge, ptr %t32, i64 %t33
  %t35 = load %struct.Edge, ptr %t34
  store %struct.Edge %t35, ptr %tmp.9
  %t36 = load ptr, ptr %edges.1
  %t37 = load i64, ptr %l.5
  %t38 = getelementptr %struct.Edge, ptr %t36, i64 %t37
  %t39 = load ptr, ptr %edges.1
  %t40 = load i64, ptr %mid.8
  %t41 = getelementptr %struct.Edge, ptr %t39, i64 %t40
  %t42 = load %struct.Edge, ptr %t41
  store %struct.Edge %t42, ptr %t38
  %t43 = load ptr, ptr %edges.1
  %t44 = load i64, ptr %mid.8
  %t45 = getelementptr %struct.Edge, ptr %t43, i64 %t44
  %t46 = load %struct.Edge, ptr %tmp.9
  store %struct.Edge %t46, ptr %t45
  br label %endif8
endif8:
  %t47 = load ptr, ptr %edges.1
  %t48 = load i64, ptr %l.5
  %t49 = getelementptr %struct.Edge, ptr %t47, i64 %t48
  %t50 = getelementptr %struct.Edge, ptr %t49, i32 0, i32 2
  %t51 = load i64, ptr %t50
  %t52 = load ptr, ptr %edges.1
  %t53 = load i64, ptr %r.6
  %t54 = getelementptr %struct.Edge, ptr %t52, i64 %t53
  %t55 = getelementptr %struct.Edge, ptr %t54, i32 0, i32 2
  %t56 = load i64, ptr %t55
  %t57 = icmp sgt i64 %t51, %t56
  br i1 %t57, label %then9, label %endif10
then9:
  %t58 = load ptr, ptr %edges.1
  %t59 = load i64, ptr %l.5
  %t60 = getelementptr %struct.Edge, ptr %t58, i64 %t59
  %t61 = load %struct.Edge, ptr %t60
  store %struct.Edge %t61, ptr %tmp.10
  %t62 = load ptr, ptr %edges.1
  %t63 = load i64, ptr %l.5
  %t64 = getelementptr %struct.Edge, ptr %t62, i64 %t63
  %t65 = load ptr, ptr %edges.1
  %t66 = load i64, ptr %r.6
  %t67 = getelementptr %struct.Edge, ptr %t65, i64 %t66
  %t68 = load %struct.Edge, ptr %t67
  store %struct.Edge %t68, ptr %t64
  %t69 = load ptr, ptr %edges.1
  %t70 = load i64, ptr %r.6
  %t71 = getelementptr %struct.Edge, ptr %t69, i64 %t70
  %t72 = load %struct.Edge, ptr %tmp.10
  store %struct.Edge %t72, ptr %t71
  br label %endif10
endif10:
  %t73 = load ptr, ptr %edges.1
  %t74 = load i64, ptr %mid.8
  %t75 = getelementptr %struct.Edge, ptr %t73, i64 %t74
  %t76 = getelementptr %struct.Edge, ptr %t75, i32 0, i32 2
  %t77 = load i64, ptr %t76
  %t78 = load ptr, ptr %edges.1
  %t79 = load i64, ptr %r.6
  %t80 = getelementptr %struct.Edge, ptr %t78, i64 %t79
  %t81 = getelementptr %struct.Edge, ptr %t80, i32 0, i32 2
  %t82 = load i64, ptr %t81
  %t83 = icmp sgt i64 %t77, %t82
  br i1 %t83, label %then11, label %endif12
then11:
  %t84 = load ptr, ptr %edges.1
  %t85 = load i64, ptr %mid.8
  %t86 = getelementptr %struct.Edge, ptr %t84, i64 %t85
  %t87 = load %struct.Edge, ptr %t86
  store %struct.Edge %t87, ptr %tmp.11
  %t88 = load ptr, ptr %edges.1
  %t89 = load i64, ptr %mid.8
  %t90 = getelementptr %struct.Edge, ptr %t88, i64 %t89
  %t91 = load ptr, ptr %edges.1
  %t92 = load i64, ptr %r.6
  %t93 = getelementptr %struct.Edge, ptr %t91, i64 %t92
  %t94 = load %struct.Edge, ptr %t93
  store %struct.Edge %t94, ptr %t90
  %t95 = load ptr, ptr %edges.1
  %t96 = load i64, ptr %r.6
  %t97 = getelementptr %struct.Edge, ptr %t95, i64 %t96
  %t98 = load %struct.Edge, ptr %tmp.11
  store %struct.Edge %t98, ptr %t97
  br label %endif12
endif12:
  %t99 = load ptr, ptr %edges.1
  %t100 = load i64, ptr %mid.8
  %t101 = getelementptr %struct.Edge, ptr %t99, i64 %t100
  %t102 = getelementptr %struct.Edge, ptr %t101, i32 0, i32 2
  %t103 = load i64, ptr %t102
  store i64 %t103, ptr %piv_w.12
  %t104 = load i64, ptr %l.5
  store i64 %t104, ptr %i.13
  %t105 = load i64, ptr %r.6
  store i64 %t105, ptr %j.14
  br label %fcond13
fcond13:
  %t106 = load i64, ptr %i.13
  %t107 = load i64, ptr %j.14
  %t108 = icmp sle i64 %t106, %t107
  br i1 %t108, label %fbody14, label %fend16
fbody14:
  br label %fcond17
fcond17:
  %t109 = load ptr, ptr %edges.1
  %t110 = load i64, ptr %i.13
  %t111 = getelementptr %struct.Edge, ptr %t109, i64 %t110
  %t112 = getelementptr %struct.Edge, ptr %t111, i32 0, i32 2
  %t113 = load i64, ptr %t112
  %t114 = load i64, ptr %piv_w.12
  %t115 = icmp slt i64 %t113, %t114
  br i1 %t115, label %fbody18, label %fend20
fbody18:
  %t116 = load i64, ptr %i.13
  %t117 = add i64 %t116, 1
  store i64 %t117, ptr %i.13
  br label %fpost19
fpost19:
  br label %fcond17
fend20:
  br label %fcond21
fcond21:
  %t118 = load ptr, ptr %edges.1
  %t119 = load i64, ptr %j.14
  %t120 = getelementptr %struct.Edge, ptr %t118, i64 %t119
  %t121 = getelementptr %struct.Edge, ptr %t120, i32 0, i32 2
  %t122 = load i64, ptr %t121
  %t123 = load i64, ptr %piv_w.12
  %t124 = icmp sgt i64 %t122, %t123
  br i1 %t124, label %fbody22, label %fend24
fbody22:
  %t125 = load i64, ptr %j.14
  %t126 = sub i64 %t125, 1
  store i64 %t126, ptr %j.14
  br label %fpost23
fpost23:
  br label %fcond21
fend24:
  %t127 = load i64, ptr %i.13
  %t128 = load i64, ptr %j.14
  %t129 = icmp sle i64 %t127, %t128
  br i1 %t129, label %then25, label %endif26
then25:
  %t130 = load ptr, ptr %edges.1
  %t131 = load i64, ptr %i.13
  %t132 = getelementptr %struct.Edge, ptr %t130, i64 %t131
  %t133 = load %struct.Edge, ptr %t132
  store %struct.Edge %t133, ptr %tmp.15
  %t134 = load ptr, ptr %edges.1
  %t135 = load i64, ptr %i.13
  %t136 = getelementptr %struct.Edge, ptr %t134, i64 %t135
  %t137 = load ptr, ptr %edges.1
  %t138 = load i64, ptr %j.14
  %t139 = getelementptr %struct.Edge, ptr %t137, i64 %t138
  %t140 = load %struct.Edge, ptr %t139
  store %struct.Edge %t140, ptr %t136
  %t141 = load ptr, ptr %edges.1
  %t142 = load i64, ptr %j.14
  %t143 = getelementptr %struct.Edge, ptr %t141, i64 %t142
  %t144 = load %struct.Edge, ptr %tmp.15
  store %struct.Edge %t144, ptr %t143
  %t145 = load i64, ptr %i.13
  %t146 = add i64 %t145, 1
  store i64 %t146, ptr %i.13
  %t147 = load i64, ptr %j.14
  %t148 = sub i64 %t147, 1
  store i64 %t148, ptr %j.14
  br label %endif26
endif26:
  br label %fpost15
fpost15:
  br label %fcond13
fend16:
  %t149 = load i64, ptr %j.14
  %t150 = load i64, ptr %l.5
  %t151 = sub i64 %t149, %t150
  %t152 = load i64, ptr %r.6
  %t153 = load i64, ptr %i.13
  %t154 = sub i64 %t152, %t153
  %t155 = icmp slt i64 %t151, %t154
  br i1 %t155, label %then27, label %else29
then27:
  %t156 = load i64, ptr %l.5
  %t157 = load i64, ptr %j.14
  %t158 = icmp slt i64 %t156, %t157
  br i1 %t158, label %then30, label %endif31
then30:
  %t159 = load ptr, ptr %edges.1
  %t160 = load i64, ptr %l.5
  %t161 = load i64, ptr %j.14
  %t162 = load i64, ptr %depth.7
  call void @introsort_edges(ptr %t159, i64 %t160, i64 %t161, i64 %t162)
  br label %endif31
endif31:
  %t163 = load i64, ptr %i.13
  store i64 %t163, ptr %l.5
  br label %endif28
else29:
  %t164 = load i64, ptr %i.13
  %t165 = load i64, ptr %r.6
  %t166 = icmp slt i64 %t164, %t165
  br i1 %t166, label %then32, label %endif33
then32:
  %t167 = load ptr, ptr %edges.1
  %t168 = load i64, ptr %i.13
  %t169 = load i64, ptr %r.6
  %t170 = load i64, ptr %depth.7
  call void @introsort_edges(ptr %t167, i64 %t168, i64 %t169, i64 %t170)
  br label %endif33
endif33:
  %t171 = load i64, ptr %j.14
  store i64 %t171, ptr %r.6
  br label %endif28
endif28:
  br label %fpost3
fpost3:
  br label %fcond1
fend4:
  %t172 = load ptr, ptr %edges.1
  %t173 = load i64, ptr %l.5
  %t174 = load i64, ptr %r.6
  call void @insertion_sort_edges(ptr %t172, i64 %t173, i64 %t174)
  ret void
}

define void @sort_edges(ptr %arg.edges, i64 %arg.left, i64 %arg.right) alwaysinline {
entry:
  %edges.1 = alloca ptr
  %left.2 = alloca i64
  %right.3 = alloca i64
  %depth.4 = alloca i64
  store ptr %arg.edges, ptr %edges.1
  store i64 %arg.left, ptr %left.2
  store i64 %arg.right, ptr %right.3
  %t1 = load i64, ptr %left.2
  %t2 = load i64, ptr %right.3
  %t3 = icmp sge i64 %t1, %t2
  br i1 %t3, label %then1, label %endif2
then1:
  ret void
endif2:
  %t4 = load i64, ptr %right.3
  %t5 = load i64, ptr %left.2
  %t6 = sub i64 %t4, %t5
  %t7 = add i64 %t6, 1
  %t8 = call i64 @calc_max_depth(i64 %t7)
  store i64 %t8, ptr %depth.4
  %t9 = load ptr, ptr %edges.1
  %t10 = load i64, ptr %left.2
  %t11 = load i64, ptr %right.3
  %t12 = load i64, ptr %depth.4
  call void @introsort_edges(ptr %t9, i64 %t10, i64 %t11, i64 %t12)
  ret void
}

define i64 @lower_bound(ptr %arg.arr, i64 %arg.len, i64 %arg.val) alwaysinline {
entry:
  %arr.1 = alloca ptr
  %len.2 = alloca i64
  %val.3 = alloca i64
  %low.4 = alloca i64
  %high.5 = alloca i64
  %ans.6 = alloca i64
  %mid.7 = alloca i64
  store ptr %arg.arr, ptr %arr.1
  store i64 %arg.len, ptr %len.2
  store i64 %arg.val, ptr %val.3
  store i64 0, ptr %low.4
  %t1 = load i64, ptr %len.2
  %t2 = sub i64 %t1, 1
  store i64 %t2, ptr %high.5
  store i64 0, ptr %ans.6
  br label %fcond1
fcond1:
  %t3 = load i64, ptr %low.4
  %t4 = load i64, ptr %high.5
  %t5 = icmp sle i64 %t3, %t4
  br i1 %t5, label %fbody2, label %fend4
fbody2:
  %t6 = load i64, ptr %low.4
  %t7 = load i64, ptr %high.5
  %t8 = load i64, ptr %low.4
  %t9 = sub i64 %t7, %t8
  %t10 = sdiv i64 %t9, 2
  %t11 = add i64 %t6, %t10
  store i64 %t11, ptr %mid.7
  %t12 = load ptr, ptr %arr.1
  %t13 = load i64, ptr %mid.7
  %t14 = getelementptr i64, ptr %t12, i64 %t13
  %t15 = load i64, ptr %t14
  %t16 = load i64, ptr %val.3
  %t17 = icmp sge i64 %t15, %t16
  br i1 %t17, label %then5, label %else7
then5:
  %t18 = load i64, ptr %mid.7
  store i64 %t18, ptr %ans.6
  %t19 = load i64, ptr %mid.7
  %t20 = sub i64 %t19, 1
  store i64 %t20, ptr %high.5
  br label %endif6
else7:
  %t21 = load i64, ptr %mid.7
  %t22 = add i64 %t21, 1
  store i64 %t22, ptr %low.4
  br label %endif6
endif6:
  br label %fpost3
fpost3:
  br label %fcond1
fend4:
  %t23 = load i64, ptr %ans.6
  ret i64 %t23
}

define i64 @dsu_find(ptr %arg.parent, i64 %arg.x) alwaysinline {
entry:
  %parent.1 = alloca ptr
  %x.2 = alloca i64
  %root.3 = alloca i64
  %curr.4 = alloca i64
  %nxt.5 = alloca i64
  store ptr %arg.parent, ptr %parent.1
  store i64 %arg.x, ptr %x.2
  %t1 = load i64, ptr %x.2
  store i64 %t1, ptr %root.3
  br label %fcond1
fcond1:
  %t2 = load ptr, ptr %parent.1
  %t3 = load i64, ptr %root.3
  %t4 = getelementptr i64, ptr %t2, i64 %t3
  %t5 = load i64, ptr %t4
  %t6 = load i64, ptr %root.3
  %t7 = icmp ne i64 %t5, %t6
  br i1 %t7, label %fbody2, label %fend4
fbody2:
  %t8 = load ptr, ptr %parent.1
  %t9 = load i64, ptr %root.3
  %t10 = getelementptr i64, ptr %t8, i64 %t9
  %t11 = load i64, ptr %t10
  store i64 %t11, ptr %root.3
  br label %fpost3
fpost3:
  br label %fcond1
fend4:
  %t12 = load i64, ptr %x.2
  store i64 %t12, ptr %curr.4
  br label %fcond5
fcond5:
  %t13 = load i64, ptr %curr.4
  %t14 = load i64, ptr %root.3
  %t15 = icmp ne i64 %t13, %t14
  br i1 %t15, label %fbody6, label %fend8
fbody6:
  %t16 = load ptr, ptr %parent.1
  %t17 = load i64, ptr %curr.4
  %t18 = getelementptr i64, ptr %t16, i64 %t17
  %t19 = load i64, ptr %t18
  store i64 %t19, ptr %nxt.5
  %t20 = load ptr, ptr %parent.1
  %t21 = load i64, ptr %curr.4
  %t22 = getelementptr i64, ptr %t20, i64 %t21
  %t23 = load i64, ptr %root.3
  store i64 %t23, ptr %t22
  %t24 = load i64, ptr %nxt.5
  store i64 %t24, ptr %curr.4
  br label %fpost7
fpost7:
  br label %fcond5
fend8:
  %t25 = load i64, ptr %root.3
  ret i64 %t25
}

define i1 @dsu_union(ptr %arg.parent, ptr %arg.rank, i64 %arg.x, i64 %arg.y) alwaysinline {
entry:
  %parent.1 = alloca ptr
  %rank.2 = alloca ptr
  %x.3 = alloca i64
  %y.4 = alloca i64
  %rx.5 = alloca i64
  %ry.6 = alloca i64
  store ptr %arg.parent, ptr %parent.1
  store ptr %arg.rank, ptr %rank.2
  store i64 %arg.x, ptr %x.3
  store i64 %arg.y, ptr %y.4
  %t1 = load ptr, ptr %parent.1
  %t2 = load i64, ptr %x.3
  %t3 = call i64 @dsu_find(ptr %t1, i64 %t2)
  store i64 %t3, ptr %rx.5
  %t4 = load ptr, ptr %parent.1
  %t5 = load i64, ptr %y.4
  %t6 = call i64 @dsu_find(ptr %t4, i64 %t5)
  store i64 %t6, ptr %ry.6
  %t7 = load i64, ptr %rx.5
  %t8 = load i64, ptr %ry.6
  %t9 = icmp eq i64 %t7, %t8
  br i1 %t9, label %then1, label %endif2
then1:
  ret i1 false
endif2:
  %t10 = load ptr, ptr %rank.2
  %t11 = load i64, ptr %rx.5
  %t12 = getelementptr i64, ptr %t10, i64 %t11
  %t13 = load i64, ptr %t12
  %t14 = load ptr, ptr %rank.2
  %t15 = load i64, ptr %ry.6
  %t16 = getelementptr i64, ptr %t14, i64 %t15
  %t17 = load i64, ptr %t16
  %t18 = icmp slt i64 %t13, %t17
  br i1 %t18, label %then3, label %else5
then3:
  %t19 = load ptr, ptr %parent.1
  %t20 = load i64, ptr %rx.5
  %t21 = getelementptr i64, ptr %t19, i64 %t20
  %t22 = load i64, ptr %ry.6
  store i64 %t22, ptr %t21
  br label %endif4
else5:
  %t23 = load ptr, ptr %rank.2
  %t24 = load i64, ptr %rx.5
  %t25 = getelementptr i64, ptr %t23, i64 %t24
  %t26 = load i64, ptr %t25
  %t27 = load ptr, ptr %rank.2
  %t28 = load i64, ptr %ry.6
  %t29 = getelementptr i64, ptr %t27, i64 %t28
  %t30 = load i64, ptr %t29
  %t31 = icmp sgt i64 %t26, %t30
  br i1 %t31, label %then6, label %else8
then6:
  %t32 = load ptr, ptr %parent.1
  %t33 = load i64, ptr %ry.6
  %t34 = getelementptr i64, ptr %t32, i64 %t33
  %t35 = load i64, ptr %rx.5
  store i64 %t35, ptr %t34
  br label %endif7
else8:
  %t36 = load ptr, ptr %parent.1
  %t37 = load i64, ptr %ry.6
  %t38 = getelementptr i64, ptr %t36, i64 %t37
  %t39 = load i64, ptr %rx.5
  store i64 %t39, ptr %t38
  %t40 = load ptr, ptr %rank.2
  %t41 = load i64, ptr %rx.5
  %t42 = getelementptr i64, ptr %t40, i64 %t41
  %t43 = load ptr, ptr %rank.2
  %t44 = load i64, ptr %rx.5
  %t45 = getelementptr i64, ptr %t43, i64 %t44
  %t46 = load i64, ptr %t45
  %t47 = add i64 %t46, 1
  store i64 %t47, ptr %t42
  br label %endif7
endif7:
  br label %endif4
endif4:
  ret i1 true
}

define i64 @solve_manhattan_mst_core(ptr %arg.orig_x, ptr %arg.orig_y, i64 %arg.n, i1 %arg.use_pure_heap) inlinehint {
entry:
  %orig_x.1 = alloca ptr
  %orig_y.2 = alloca ptr
  %n.3 = alloca i64
  %use_pure_heap.4 = alloca i1
  %ret.5 = alloca %struct.BufferGuard
  %pts_buf.6 = alloca %struct.BufferGuard
  %pts.7 = alloca ptr
  %ret.8 = alloca %struct.BufferGuard
  %z_buf.9 = alloca %struct.BufferGuard
  %z_vals.10 = alloca ptr
  %ret.11 = alloca %struct.BufferGuard
  %bit_val_buf.12 = alloca %struct.BufferGuard
  %bit_val.13 = alloca ptr
  %ret.14 = alloca %struct.BufferGuard
  %bit_id_buf.15 = alloca %struct.BufferGuard
  %bit_id.16 = alloca ptr
  %max_edges.17 = alloca i64
  %ret.18 = alloca %struct.BufferGuard
  %edges_buf.19 = alloca %struct.BufferGuard
  %edges.20 = alloca ptr
  %edge_count.21 = alloca i64
  %i.22 = alloca i64
  %lit_Point.23 = alloca %struct.Point
  %inf.24 = alloca i64
  %dir.25 = alloca i64
  %logic.26 = alloca i1
  %i.27 = alloca i64
  %tmp.28 = alloca i64
  %i.29 = alloca i64
  %i.30 = alloca i64
  %m.31 = alloca i64
  %i.32 = alloca i64
  %p.33 = alloca i64
  %i.34 = alloca i64
  %z.35 = alloca i64
  %rank.36 = alloca i64
  %pos.37 = alloca i64
  %best_id.38 = alloca i64
  %min_val.39 = alloca i64
  %p.40 = alloca i64
  %lowbit.41 = alloca i64
  %u.42 = alloca i64
  %v.43 = alloca i64
  %dist.44 = alloca i64
  %lit_Edge.45 = alloca %struct.Edge
  %val.46 = alloca i64
  %id.47 = alloca i64
  %up.48 = alloca i64
  %lowbit.49 = alloca i64
  %ret.50 = alloca %struct.BufferGuard
  %parent_buf.51 = alloca %struct.BufferGuard
  %parent.52 = alloca ptr
  %ret.53 = alloca %struct.BufferGuard
  %rank_buf.54 = alloca %struct.BufferGuard
  %rank.55 = alloca ptr
  %i.56 = alloca i64
  %total_mst_weight.57 = alloca i64
  %edges_added.58 = alloca i64
  %i.59 = alloca i64
  %e.60 = alloca %struct.Edge
  store ptr %arg.orig_x, ptr %orig_x.1
  store ptr %arg.orig_y, ptr %orig_y.2
  store i64 %arg.n, ptr %n.3
  store i1 %arg.use_pure_heap, ptr %use_pure_heap.4
  %t1 = load i64, ptr %n.3
  %t2 = icmp sle i64 %t1, 1
  br i1 %t2, label %then1, label %endif2
then1:
  ret i64 0
endif2:
  %t3 = load ptr, ptr %orig_x.1
  %t4 = load ptr, ptr %orig_y.2
  %t5 = load i64, ptr %n.3
  %t6 = call i1 @validate_coordinates(ptr %t3, ptr %t4, i64 %t5)
  %t7 = xor i1 %t6, true
  br i1 %t7, label %then3, label %endif4
then3:
  %t8 = call i32 (ptr, ...) @printf(ptr @.str.0, i64 -1000000000, i64 1000000000)
  %t9 = sub i64 0, 1
  ret i64 %t9
endif4:
  %t10 = load i64, ptr %n.3
  %t11 = getelementptr %struct.Point, ptr null, i64 1
  %t12 = ptrtoint ptr %t11 to i64
  %t13 = mul i64 %t10, %t12
  %t14 = call %struct.BufferGuard @BufferGuard__new(i64 %t13)
  store %struct.BufferGuard %t14, ptr %ret.5
  %t15 = load %struct.BufferGuard, ptr %ret.5
  store %struct.BufferGuard %t15, ptr %pts_buf.6
  %t16 = getelementptr %struct.BufferGuard, ptr %pts_buf.6, i32 0, i32 0
  %t17 = load ptr, ptr %t16
  store ptr %t17, ptr %pts.7
  %t18 = load i64, ptr %n.3
  %t19 = getelementptr i64, ptr null, i64 1
  %t20 = ptrtoint ptr %t19 to i64
  %t21 = mul i64 %t18, %t20
  %t22 = call %struct.BufferGuard @BufferGuard__new(i64 %t21)
  store %struct.BufferGuard %t22, ptr %ret.8
  %t23 = load %struct.BufferGuard, ptr %ret.8
  store %struct.BufferGuard %t23, ptr %z_buf.9
  %t24 = getelementptr %struct.BufferGuard, ptr %z_buf.9, i32 0, i32 0
  %t25 = load ptr, ptr %t24
  store ptr %t25, ptr %z_vals.10
  %t26 = load i64, ptr %n.3
  %t27 = add i64 %t26, 4
  %t28 = getelementptr i64, ptr null, i64 1
  %t29 = ptrtoint ptr %t28 to i64
  %t30 = mul i64 %t27, %t29
  %t31 = call %struct.BufferGuard @BufferGuard__new(i64 %t30)
  store %struct.BufferGuard %t31, ptr %ret.11
  %t32 = load %struct.BufferGuard, ptr %ret.11
  store %struct.BufferGuard %t32, ptr %bit_val_buf.12
  %t33 = getelementptr %struct.BufferGuard, ptr %bit_val_buf.12, i32 0, i32 0
  %t34 = load ptr, ptr %t33
  store ptr %t34, ptr %bit_val.13
  %t35 = load i64, ptr %n.3
  %t36 = add i64 %t35, 4
  %t37 = getelementptr i64, ptr null, i64 1
  %t38 = ptrtoint ptr %t37 to i64
  %t39 = mul i64 %t36, %t38
  %t40 = call %struct.BufferGuard @BufferGuard__new(i64 %t39)
  store %struct.BufferGuard %t40, ptr %ret.14
  %t41 = load %struct.BufferGuard, ptr %ret.14
  store %struct.BufferGuard %t41, ptr %bit_id_buf.15
  %t42 = getelementptr %struct.BufferGuard, ptr %bit_id_buf.15, i32 0, i32 0
  %t43 = load ptr, ptr %t42
  store ptr %t43, ptr %bit_id.16
  %t44 = load i64, ptr %n.3
  %t45 = mul i64 4, %t44
  %t46 = add i64 %t45, 10
  store i64 %t46, ptr %max_edges.17
  %t47 = load i64, ptr %max_edges.17
  %t48 = getelementptr %struct.Edge, ptr null, i64 1
  %t49 = ptrtoint ptr %t48 to i64
  %t50 = mul i64 %t47, %t49
  %t51 = call %struct.BufferGuard @BufferGuard__new(i64 %t50)
  store %struct.BufferGuard %t51, ptr %ret.18
  %t52 = load %struct.BufferGuard, ptr %ret.18
  store %struct.BufferGuard %t52, ptr %edges_buf.19
  %t53 = getelementptr %struct.BufferGuard, ptr %edges_buf.19, i32 0, i32 0
  %t54 = load ptr, ptr %t53
  store ptr %t54, ptr %edges.20
  store i64 0, ptr %edge_count.21
  store i64 0, ptr %i.22
  br label %fcond5
fcond5:
  %t55 = load i64, ptr %i.22
  %t56 = load i64, ptr %n.3
  %t57 = icmp slt i64 %t55, %t56
  br i1 %t57, label %fbody6, label %fend8
fbody6:
  %t58 = load ptr, ptr %pts.7
  %t59 = load i64, ptr %i.22
  %t60 = getelementptr %struct.Point, ptr %t58, i64 %t59
  %t61 = load ptr, ptr %orig_x.1
  %t62 = load i64, ptr %i.22
  %t63 = getelementptr i64, ptr %t61, i64 %t62
  %t64 = load i64, ptr %t63
  %t65 = getelementptr %struct.Point, ptr %lit_Point.23, i32 0, i32 0
  store i64 %t64, ptr %t65
  %t66 = load ptr, ptr %orig_y.2
  %t67 = load i64, ptr %i.22
  %t68 = getelementptr i64, ptr %t66, i64 %t67
  %t69 = load i64, ptr %t68
  %t70 = getelementptr %struct.Point, ptr %lit_Point.23, i32 0, i32 1
  store i64 %t69, ptr %t70
  %t71 = load i64, ptr %i.22
  %t72 = getelementptr %struct.Point, ptr %lit_Point.23, i32 0, i32 2
  store i64 %t71, ptr %t72
  %t73 = load %struct.Point, ptr %lit_Point.23
  store %struct.Point %t73, ptr %t60
  br label %fpost7
fpost7:
  %t74 = load i64, ptr %i.22
  %t75 = add i64 %t74, 1
  store i64 %t75, ptr %i.22
  br label %fcond5
fend8:
  store i64 4000000000000000000, ptr %inf.24
  store i64 0, ptr %dir.25
  br label %fcond9
fcond9:
  %t76 = load i64, ptr %dir.25
  %t77 = icmp slt i64 %t76, 4
  br i1 %t77, label %fbody10, label %fend12
fbody10:
  %t78 = load i64, ptr %dir.25
  %t79 = icmp eq i64 %t78, 1
  store i1 %t79, ptr %logic.26
  br i1 %t79, label %logdone14, label %logrhs13
logrhs13:
  %t80 = load i64, ptr %dir.25
  %t81 = icmp eq i64 %t80, 3
  store i1 %t81, ptr %logic.26
  br label %logdone14
logdone14:
  %t82 = load i1, ptr %logic.26
  br i1 %t82, label %then15, label %else17
then15:
  store i64 0, ptr %i.27
  br label %fcond18
fcond18:
  %t83 = load i64, ptr %i.27
  %t84 = load i64, ptr %n.3
  %t85 = icmp slt i64 %t83, %t84
  br i1 %t85, label %fbody19, label %fend21
fbody19:
  %t86 = load ptr, ptr %pts.7
  %t87 = load i64, ptr %i.27
  %t88 = getelementptr %struct.Point, ptr %t86, i64 %t87
  %t89 = getelementptr %struct.Point, ptr %t88, i32 0, i32 0
  %t90 = load i64, ptr %t89
  store i64 %t90, ptr %tmp.28
  %t91 = load ptr, ptr %pts.7
  %t92 = load i64, ptr %i.27
  %t93 = getelementptr %struct.Point, ptr %t91, i64 %t92
  %t94 = getelementptr %struct.Point, ptr %t93, i32 0, i32 0
  %t95 = load ptr, ptr %pts.7
  %t96 = load i64, ptr %i.27
  %t97 = getelementptr %struct.Point, ptr %t95, i64 %t96
  %t98 = getelementptr %struct.Point, ptr %t97, i32 0, i32 1
  %t99 = load i64, ptr %t98
  store i64 %t99, ptr %t94
  %t100 = load ptr, ptr %pts.7
  %t101 = load i64, ptr %i.27
  %t102 = getelementptr %struct.Point, ptr %t100, i64 %t101
  %t103 = getelementptr %struct.Point, ptr %t102, i32 0, i32 1
  %t104 = load i64, ptr %tmp.28
  store i64 %t104, ptr %t103
  br label %fpost20
fpost20:
  %t105 = load i64, ptr %i.27
  %t106 = add i64 %t105, 1
  store i64 %t106, ptr %i.27
  br label %fcond18
fend21:
  br label %endif16
else17:
  %t107 = load i64, ptr %dir.25
  %t108 = icmp eq i64 %t107, 2
  br i1 %t108, label %then22, label %endif23
then22:
  store i64 0, ptr %i.29
  br label %fcond24
fcond24:
  %t109 = load i64, ptr %i.29
  %t110 = load i64, ptr %n.3
  %t111 = icmp slt i64 %t109, %t110
  br i1 %t111, label %fbody25, label %fend27
fbody25:
  %t112 = load ptr, ptr %pts.7
  %t113 = load i64, ptr %i.29
  %t114 = getelementptr %struct.Point, ptr %t112, i64 %t113
  %t115 = getelementptr %struct.Point, ptr %t114, i32 0, i32 0
  %t116 = load ptr, ptr %pts.7
  %t117 = load i64, ptr %i.29
  %t118 = getelementptr %struct.Point, ptr %t116, i64 %t117
  %t119 = getelementptr %struct.Point, ptr %t118, i32 0, i32 0
  %t120 = load i64, ptr %t119
  %t121 = sub i64 0, %t120
  store i64 %t121, ptr %t115
  br label %fpost26
fpost26:
  %t122 = load i64, ptr %i.29
  %t123 = add i64 %t122, 1
  store i64 %t123, ptr %i.29
  br label %fcond24
fend27:
  br label %endif23
endif23:
  br label %endif16
endif16:
  %t124 = load i1, ptr %use_pure_heap.4
  br i1 %t124, label %then28, label %else30
then28:
  %t125 = load ptr, ptr %pts.7
  %t126 = load i64, ptr %n.3
  %t127 = sub i64 %t126, 1
  call void @heapsort_points(ptr %t125, i64 0, i64 %t127)
  br label %endif29
else30:
  %t128 = load ptr, ptr %pts.7
  %t129 = load i64, ptr %n.3
  %t130 = sub i64 %t129, 1
  call void @sort_points(ptr %t128, i64 0, i64 %t130)
  br label %endif29
endif29:
  store i64 0, ptr %i.30
  br label %fcond31
fcond31:
  %t131 = load i64, ptr %i.30
  %t132 = load i64, ptr %n.3
  %t133 = icmp slt i64 %t131, %t132
  br i1 %t133, label %fbody32, label %fend34
fbody32:
  %t134 = load ptr, ptr %z_vals.10
  %t135 = load i64, ptr %i.30
  %t136 = getelementptr i64, ptr %t134, i64 %t135
  %t137 = load ptr, ptr %pts.7
  %t138 = load i64, ptr %i.30
  %t139 = getelementptr %struct.Point, ptr %t137, i64 %t138
  %t140 = getelementptr %struct.Point, ptr %t139, i32 0, i32 1
  %t141 = load i64, ptr %t140
  %t142 = load ptr, ptr %pts.7
  %t143 = load i64, ptr %i.30
  %t144 = getelementptr %struct.Point, ptr %t142, i64 %t143
  %t145 = getelementptr %struct.Point, ptr %t144, i32 0, i32 0
  %t146 = load i64, ptr %t145
  %t147 = sub i64 %t141, %t146
  store i64 %t147, ptr %t136
  br label %fpost33
fpost33:
  %t148 = load i64, ptr %i.30
  %t149 = add i64 %t148, 1
  store i64 %t149, ptr %i.30
  br label %fcond31
fend34:
  %t150 = load i1, ptr %use_pure_heap.4
  br i1 %t150, label %then35, label %else37
then35:
  %t151 = load ptr, ptr %z_vals.10
  %t152 = load i64, ptr %n.3
  %t153 = sub i64 %t152, 1
  call void @heapsort_i64(ptr %t151, i64 0, i64 %t153)
  br label %endif36
else37:
  %t154 = load ptr, ptr %z_vals.10
  %t155 = load i64, ptr %n.3
  %t156 = sub i64 %t155, 1
  call void @sort_i64(ptr %t154, i64 0, i64 %t156)
  br label %endif36
endif36:
  store i64 0, ptr %m.31
  %t157 = load i64, ptr %n.3
  %t158 = icmp sgt i64 %t157, 0
  br i1 %t158, label %then38, label %endif39
then38:
  store i64 1, ptr %m.31
  store i64 1, ptr %i.32
  br label %fcond40
fcond40:
  %t159 = load i64, ptr %i.32
  %t160 = load i64, ptr %n.3
  %t161 = icmp slt i64 %t159, %t160
  br i1 %t161, label %fbody41, label %fend43
fbody41:
  %t162 = load ptr, ptr %z_vals.10
  %t163 = load i64, ptr %i.32
  %t164 = getelementptr i64, ptr %t162, i64 %t163
  %t165 = load i64, ptr %t164
  %t166 = load ptr, ptr %z_vals.10
  %t167 = load i64, ptr %m.31
  %t168 = sub i64 %t167, 1
  %t169 = getelementptr i64, ptr %t166, i64 %t168
  %t170 = load i64, ptr %t169
  %t171 = icmp ne i64 %t165, %t170
  br i1 %t171, label %then44, label %endif45
then44:
  %t172 = load ptr, ptr %z_vals.10
  %t173 = load i64, ptr %m.31
  %t174 = getelementptr i64, ptr %t172, i64 %t173
  %t175 = load ptr, ptr %z_vals.10
  %t176 = load i64, ptr %i.32
  %t177 = getelementptr i64, ptr %t175, i64 %t176
  %t178 = load i64, ptr %t177
  store i64 %t178, ptr %t174
  %t179 = load i64, ptr %m.31
  %t180 = add i64 %t179, 1
  store i64 %t180, ptr %m.31
  br label %endif45
endif45:
  br label %fpost42
fpost42:
  %t181 = load i64, ptr %i.32
  %t182 = add i64 %t181, 1
  store i64 %t182, ptr %i.32
  br label %fcond40
fend43:
  br label %endif39
endif39:
  store i64 0, ptr %p.33
  br label %fcond46
fcond46:
  %t183 = load i64, ptr %p.33
  %t184 = load i64, ptr %m.31
  %t185 = add i64 %t184, 2
  %t186 = icmp sle i64 %t183, %t185
  br i1 %t186, label %fbody47, label %fend49
fbody47:
  %t187 = load ptr, ptr %bit_val.13
  %t188 = load i64, ptr %p.33
  %t189 = getelementptr i64, ptr %t187, i64 %t188
  %t190 = load i64, ptr %inf.24
  store i64 %t190, ptr %t189
  %t191 = load ptr, ptr %bit_id.16
  %t192 = load i64, ptr %p.33
  %t193 = getelementptr i64, ptr %t191, i64 %t192
  %t194 = sub i64 0, 1
  store i64 %t194, ptr %t193
  br label %fpost48
fpost48:
  %t195 = load i64, ptr %p.33
  %t196 = add i64 %t195, 1
  store i64 %t196, ptr %p.33
  br label %fcond46
fend49:
  store i64 0, ptr %i.34
  br label %fcond50
fcond50:
  %t197 = load i64, ptr %i.34
  %t198 = load i64, ptr %n.3
  %t199 = icmp slt i64 %t197, %t198
  br i1 %t199, label %fbody51, label %fend53
fbody51:
  %t200 = load ptr, ptr %pts.7
  %t201 = load i64, ptr %i.34
  %t202 = getelementptr %struct.Point, ptr %t200, i64 %t201
  %t203 = getelementptr %struct.Point, ptr %t202, i32 0, i32 1
  %t204 = load i64, ptr %t203
  %t205 = load ptr, ptr %pts.7
  %t206 = load i64, ptr %i.34
  %t207 = getelementptr %struct.Point, ptr %t205, i64 %t206
  %t208 = getelementptr %struct.Point, ptr %t207, i32 0, i32 0
  %t209 = load i64, ptr %t208
  %t210 = sub i64 %t204, %t209
  store i64 %t210, ptr %z.35
  %t211 = load ptr, ptr %z_vals.10
  %t212 = load i64, ptr %m.31
  %t213 = load i64, ptr %z.35
  %t214 = call i64 @lower_bound(ptr %t211, i64 %t212, i64 %t213)
  store i64 %t214, ptr %rank.36
  %t215 = load i64, ptr %m.31
  %t216 = load i64, ptr %rank.36
  %t217 = sub i64 %t215, %t216
  store i64 %t217, ptr %pos.37
  %t218 = sub i64 0, 1
  store i64 %t218, ptr %best_id.38
  %t219 = load i64, ptr %inf.24
  store i64 %t219, ptr %min_val.39
  %t220 = load i64, ptr %pos.37
  store i64 %t220, ptr %p.40
  br label %fcond54
fcond54:
  %t221 = load i64, ptr %p.40
  %t222 = icmp sgt i64 %t221, 0
  br i1 %t222, label %fbody55, label %fend57
fbody55:
  %t223 = load ptr, ptr %bit_val.13
  %t224 = load i64, ptr %p.40
  %t225 = getelementptr i64, ptr %t223, i64 %t224
  %t226 = load i64, ptr %t225
  %t227 = load i64, ptr %min_val.39
  %t228 = icmp slt i64 %t226, %t227
  br i1 %t228, label %then58, label %endif59
then58:
  %t229 = load ptr, ptr %bit_val.13
  %t230 = load i64, ptr %p.40
  %t231 = getelementptr i64, ptr %t229, i64 %t230
  %t232 = load i64, ptr %t231
  store i64 %t232, ptr %min_val.39
  %t233 = load ptr, ptr %bit_id.16
  %t234 = load i64, ptr %p.40
  %t235 = getelementptr i64, ptr %t233, i64 %t234
  %t236 = load i64, ptr %t235
  store i64 %t236, ptr %best_id.38
  br label %endif59
endif59:
  %t237 = load i64, ptr %p.40
  %t238 = load i64, ptr %p.40
  %t239 = sub i64 0, %t238
  %t240 = and i64 %t237, %t239
  store i64 %t240, ptr %lowbit.41
  %t241 = load i64, ptr %p.40
  %t242 = load i64, ptr %lowbit.41
  %t243 = sub i64 %t241, %t242
  store i64 %t243, ptr %p.40
  br label %fpost56
fpost56:
  br label %fcond54
fend57:
  %t244 = load i64, ptr %best_id.38
  %t245 = sub i64 0, 1
  %t246 = icmp ne i64 %t244, %t245
  br i1 %t246, label %then60, label %endif61
then60:
  %t247 = load ptr, ptr %pts.7
  %t248 = load i64, ptr %i.34
  %t249 = getelementptr %struct.Point, ptr %t247, i64 %t248
  %t250 = getelementptr %struct.Point, ptr %t249, i32 0, i32 2
  %t251 = load i64, ptr %t250
  store i64 %t251, ptr %u.42
  %t252 = load i64, ptr %best_id.38
  store i64 %t252, ptr %v.43
  %t253 = load ptr, ptr %orig_x.1
  %t254 = load i64, ptr %u.42
  %t255 = getelementptr i64, ptr %t253, i64 %t254
  %t256 = load i64, ptr %t255
  %t257 = load ptr, ptr %orig_x.1
  %t258 = load i64, ptr %v.43
  %t259 = getelementptr i64, ptr %t257, i64 %t258
  %t260 = load i64, ptr %t259
  %t261 = sub i64 %t256, %t260
  %t262 = call i64 @abs_i64(i64 %t261)
  %t263 = load ptr, ptr %orig_y.2
  %t264 = load i64, ptr %u.42
  %t265 = getelementptr i64, ptr %t263, i64 %t264
  %t266 = load i64, ptr %t265
  %t267 = load ptr, ptr %orig_y.2
  %t268 = load i64, ptr %v.43
  %t269 = getelementptr i64, ptr %t267, i64 %t268
  %t270 = load i64, ptr %t269
  %t271 = sub i64 %t266, %t270
  %t272 = call i64 @abs_i64(i64 %t271)
  %t273 = add i64 %t262, %t272
  store i64 %t273, ptr %dist.44
  %t274 = load ptr, ptr %edges.20
  %t275 = load i64, ptr %edge_count.21
  %t276 = getelementptr %struct.Edge, ptr %t274, i64 %t275
  %t277 = load i64, ptr %u.42
  %t278 = getelementptr %struct.Edge, ptr %lit_Edge.45, i32 0, i32 0
  store i64 %t277, ptr %t278
  %t279 = load i64, ptr %v.43
  %t280 = getelementptr %struct.Edge, ptr %lit_Edge.45, i32 0, i32 1
  store i64 %t279, ptr %t280
  %t281 = load i64, ptr %dist.44
  %t282 = getelementptr %struct.Edge, ptr %lit_Edge.45, i32 0, i32 2
  store i64 %t281, ptr %t282
  %t283 = load %struct.Edge, ptr %lit_Edge.45
  store %struct.Edge %t283, ptr %t276
  %t284 = load i64, ptr %edge_count.21
  %t285 = add i64 %t284, 1
  store i64 %t285, ptr %edge_count.21
  br label %endif61
endif61:
  %t286 = load ptr, ptr %pts.7
  %t287 = load i64, ptr %i.34
  %t288 = getelementptr %struct.Point, ptr %t286, i64 %t287
  %t289 = getelementptr %struct.Point, ptr %t288, i32 0, i32 0
  %t290 = load i64, ptr %t289
  %t291 = load ptr, ptr %pts.7
  %t292 = load i64, ptr %i.34
  %t293 = getelementptr %struct.Point, ptr %t291, i64 %t292
  %t294 = getelementptr %struct.Point, ptr %t293, i32 0, i32 1
  %t295 = load i64, ptr %t294
  %t296 = add i64 %t290, %t295
  store i64 %t296, ptr %val.46
  %t297 = load ptr, ptr %pts.7
  %t298 = load i64, ptr %i.34
  %t299 = getelementptr %struct.Point, ptr %t297, i64 %t298
  %t300 = getelementptr %struct.Point, ptr %t299, i32 0, i32 2
  %t301 = load i64, ptr %t300
  store i64 %t301, ptr %id.47
  %t302 = load i64, ptr %pos.37
  store i64 %t302, ptr %up.48
  br label %fcond62
fcond62:
  %t303 = load i64, ptr %up.48
  %t304 = load i64, ptr %m.31
  %t305 = icmp sle i64 %t303, %t304
  br i1 %t305, label %fbody63, label %fend65
fbody63:
  %t306 = load i64, ptr %val.46
  %t307 = load ptr, ptr %bit_val.13
  %t308 = load i64, ptr %up.48
  %t309 = getelementptr i64, ptr %t307, i64 %t308
  %t310 = load i64, ptr %t309
  %t311 = icmp slt i64 %t306, %t310
  br i1 %t311, label %then66, label %endif67
then66:
  %t312 = load ptr, ptr %bit_val.13
  %t313 = load i64, ptr %up.48
  %t314 = getelementptr i64, ptr %t312, i64 %t313
  %t315 = load i64, ptr %val.46
  store i64 %t315, ptr %t314
  %t316 = load ptr, ptr %bit_id.16
  %t317 = load i64, ptr %up.48
  %t318 = getelementptr i64, ptr %t316, i64 %t317
  %t319 = load i64, ptr %id.47
  store i64 %t319, ptr %t318
  br label %endif67
endif67:
  %t320 = load i64, ptr %up.48
  %t321 = load i64, ptr %up.48
  %t322 = sub i64 0, %t321
  %t323 = and i64 %t320, %t322
  store i64 %t323, ptr %lowbit.49
  %t324 = load i64, ptr %up.48
  %t325 = load i64, ptr %lowbit.49
  %t326 = add i64 %t324, %t325
  store i64 %t326, ptr %up.48
  br label %fpost64
fpost64:
  br label %fcond62
fend65:
  br label %fpost52
fpost52:
  %t327 = load i64, ptr %i.34
  %t328 = add i64 %t327, 1
  store i64 %t328, ptr %i.34
  br label %fcond50
fend53:
  br label %fpost11
fpost11:
  %t329 = load i64, ptr %dir.25
  %t330 = add i64 %t329, 1
  store i64 %t330, ptr %dir.25
  br label %fcond9
fend12:
  %t331 = load i64, ptr %edge_count.21
  %t332 = icmp sgt i64 %t331, 0
  br i1 %t332, label %then68, label %endif69
then68:
  %t333 = load i1, ptr %use_pure_heap.4
  br i1 %t333, label %then70, label %else72
then70:
  %t334 = load ptr, ptr %edges.20
  %t335 = load i64, ptr %edge_count.21
  %t336 = sub i64 %t335, 1
  call void @heapsort_edges(ptr %t334, i64 0, i64 %t336)
  br label %endif71
else72:
  %t337 = load ptr, ptr %edges.20
  %t338 = load i64, ptr %edge_count.21
  %t339 = sub i64 %t338, 1
  call void @sort_edges(ptr %t337, i64 0, i64 %t339)
  br label %endif71
endif71:
  br label %endif69
endif69:
  %t340 = load i64, ptr %n.3
  %t341 = getelementptr i64, ptr null, i64 1
  %t342 = ptrtoint ptr %t341 to i64
  %t343 = mul i64 %t340, %t342
  %t344 = call %struct.BufferGuard @BufferGuard__new(i64 %t343)
  store %struct.BufferGuard %t344, ptr %ret.50
  %t345 = load %struct.BufferGuard, ptr %ret.50
  store %struct.BufferGuard %t345, ptr %parent_buf.51
  %t346 = getelementptr %struct.BufferGuard, ptr %parent_buf.51, i32 0, i32 0
  %t347 = load ptr, ptr %t346
  store ptr %t347, ptr %parent.52
  %t348 = load i64, ptr %n.3
  %t349 = getelementptr i64, ptr null, i64 1
  %t350 = ptrtoint ptr %t349 to i64
  %t351 = mul i64 %t348, %t350
  %t352 = call %struct.BufferGuard @BufferGuard__new(i64 %t351)
  store %struct.BufferGuard %t352, ptr %ret.53
  %t353 = load %struct.BufferGuard, ptr %ret.53
  store %struct.BufferGuard %t353, ptr %rank_buf.54
  %t354 = getelementptr %struct.BufferGuard, ptr %rank_buf.54, i32 0, i32 0
  %t355 = load ptr, ptr %t354
  store ptr %t355, ptr %rank.55
  store i64 0, ptr %i.56
  br label %fcond73
fcond73:
  %t356 = load i64, ptr %i.56
  %t357 = load i64, ptr %n.3
  %t358 = icmp slt i64 %t356, %t357
  br i1 %t358, label %fbody74, label %fend76
fbody74:
  %t359 = load ptr, ptr %parent.52
  %t360 = load i64, ptr %i.56
  %t361 = getelementptr i64, ptr %t359, i64 %t360
  %t362 = load i64, ptr %i.56
  store i64 %t362, ptr %t361
  %t363 = load ptr, ptr %rank.55
  %t364 = load i64, ptr %i.56
  %t365 = getelementptr i64, ptr %t363, i64 %t364
  store i64 0, ptr %t365
  br label %fpost75
fpost75:
  %t366 = load i64, ptr %i.56
  %t367 = add i64 %t366, 1
  store i64 %t367, ptr %i.56
  br label %fcond73
fend76:
  store i64 0, ptr %total_mst_weight.57
  store i64 0, ptr %edges_added.58
  store i64 0, ptr %i.59
  br label %fcond77
fcond77:
  %t368 = load i64, ptr %i.59
  %t369 = load i64, ptr %edge_count.21
  %t370 = icmp slt i64 %t368, %t369
  br i1 %t370, label %fbody78, label %fend80
fbody78:
  %t371 = load ptr, ptr %edges.20
  %t372 = load i64, ptr %i.59
  %t373 = getelementptr %struct.Edge, ptr %t371, i64 %t372
  %t374 = load %struct.Edge, ptr %t373
  store %struct.Edge %t374, ptr %e.60
  %t375 = load ptr, ptr %parent.52
  %t376 = load ptr, ptr %rank.55
  %t377 = getelementptr %struct.Edge, ptr %e.60, i32 0, i32 0
  %t378 = load i64, ptr %t377
  %t379 = getelementptr %struct.Edge, ptr %e.60, i32 0, i32 1
  %t380 = load i64, ptr %t379
  %t381 = call i1 @dsu_union(ptr %t375, ptr %t376, i64 %t378, i64 %t380)
  br i1 %t381, label %then81, label %endif82
then81:
  %t382 = load i64, ptr %total_mst_weight.57
  %t383 = getelementptr %struct.Edge, ptr %e.60, i32 0, i32 2
  %t384 = load i64, ptr %t383
  %t385 = add i64 %t382, %t384
  store i64 %t385, ptr %total_mst_weight.57
  %t386 = load i64, ptr %edges_added.58
  %t387 = add i64 %t386, 1
  store i64 %t387, ptr %edges_added.58
  %t388 = load i64, ptr %edges_added.58
  %t389 = load i64, ptr %n.3
  %t390 = sub i64 %t389, 1
  %t391 = icmp eq i64 %t388, %t390
  br i1 %t391, label %then83, label %endif84
then83:
  br label %fend80
endif84:
  br label %endif82
endif82:
  br label %fpost79
fpost79:
  %t392 = load i64, ptr %i.59
  %t393 = add i64 %t392, 1
  store i64 %t393, ptr %i.59
  br label %fcond77
fend80:
  %t394 = load i64, ptr %total_mst_weight.57
  call void @BufferGuard__drop(ptr %rank_buf.54)
  call void @BufferGuard__drop(ptr %parent_buf.51)
  call void @BufferGuard__drop(ptr %edges_buf.19)
  call void @BufferGuard__drop(ptr %bit_id_buf.15)
  call void @BufferGuard__drop(ptr %bit_val_buf.12)
  call void @BufferGuard__drop(ptr %z_buf.9)
  call void @BufferGuard__drop(ptr %pts_buf.6)
  ret i64 %t394
}

define i64 @solve_manhattan_mst(ptr %arg.orig_x, ptr %arg.orig_y, i64 %arg.n) alwaysinline {
entry:
  %orig_x.1 = alloca ptr
  %orig_y.2 = alloca ptr
  %n.3 = alloca i64
  store ptr %arg.orig_x, ptr %orig_x.1
  store ptr %arg.orig_y, ptr %orig_y.2
  store i64 %arg.n, ptr %n.3
  %t1 = load ptr, ptr %orig_x.1
  %t2 = load ptr, ptr %orig_y.2
  %t3 = load i64, ptr %n.3
  %t4 = call i64 @solve_manhattan_mst_core(ptr %t1, ptr %t2, i64 %t3, i1 false)
  ret i64 %t4
}

define i64 @solve_manhattan_mst_pure_heapsort(ptr %arg.orig_x, ptr %arg.orig_y, i64 %arg.n) alwaysinline {
entry:
  %orig_x.1 = alloca ptr
  %orig_y.2 = alloca ptr
  %n.3 = alloca i64
  store ptr %arg.orig_x, ptr %orig_x.1
  store ptr %arg.orig_y, ptr %orig_y.2
  store i64 %arg.n, ptr %n.3
  %t1 = load ptr, ptr %orig_x.1
  %t2 = load ptr, ptr %orig_y.2
  %t3 = load i64, ptr %n.3
  %t4 = call i64 @solve_manhattan_mst_core(ptr %t1, ptr %t2, i64 %t3, i1 true)
  ret i64 %t4
}

define i64 @solve_manhattan_mst_bruteforce(ptr %arg.orig_x, ptr %arg.orig_y, i64 %arg.n) inlinehint {
entry:
  %orig_x.1 = alloca ptr
  %orig_y.2 = alloca ptr
  %n.3 = alloca i64
  %ret.4 = alloca %struct.BufferGuard
  %min_dist_buf.5 = alloca %struct.BufferGuard
  %min_dist.6 = alloca ptr
  %ret.7 = alloca %struct.BufferGuard
  %visited_buf.8 = alloca %struct.BufferGuard
  %visited.9 = alloca ptr
  %inf.10 = alloca i64
  %i.11 = alloca i64
  %total_weight.12 = alloca i64
  %step.13 = alloca i64
  %u.14 = alloca i64
  %best_d.15 = alloca i64
  %i.16 = alloca i64
  %logic.17 = alloca i1
  %v.18 = alloca i64
  %d.19 = alloca i64
  store ptr %arg.orig_x, ptr %orig_x.1
  store ptr %arg.orig_y, ptr %orig_y.2
  store i64 %arg.n, ptr %n.3
  %t1 = load i64, ptr %n.3
  %t2 = icmp sle i64 %t1, 1
  br i1 %t2, label %then1, label %endif2
then1:
  ret i64 0
endif2:
  %t3 = load i64, ptr %n.3
  %t4 = getelementptr i64, ptr null, i64 1
  %t5 = ptrtoint ptr %t4 to i64
  %t6 = mul i64 %t3, %t5
  %t7 = call %struct.BufferGuard @BufferGuard__new(i64 %t6)
  store %struct.BufferGuard %t7, ptr %ret.4
  %t8 = load %struct.BufferGuard, ptr %ret.4
  store %struct.BufferGuard %t8, ptr %min_dist_buf.5
  %t9 = getelementptr %struct.BufferGuard, ptr %min_dist_buf.5, i32 0, i32 0
  %t10 = load ptr, ptr %t9
  store ptr %t10, ptr %min_dist.6
  %t11 = load i64, ptr %n.3
  %t12 = getelementptr i1, ptr null, i64 1
  %t13 = ptrtoint ptr %t12 to i64
  %t14 = mul i64 %t11, %t13
  %t15 = call %struct.BufferGuard @BufferGuard__new(i64 %t14)
  store %struct.BufferGuard %t15, ptr %ret.7
  %t16 = load %struct.BufferGuard, ptr %ret.7
  store %struct.BufferGuard %t16, ptr %visited_buf.8
  %t17 = getelementptr %struct.BufferGuard, ptr %visited_buf.8, i32 0, i32 0
  %t18 = load ptr, ptr %t17
  store ptr %t18, ptr %visited.9
  store i64 4000000000000000000, ptr %inf.10
  store i64 0, ptr %i.11
  br label %fcond3
fcond3:
  %t19 = load i64, ptr %i.11
  %t20 = load i64, ptr %n.3
  %t21 = icmp slt i64 %t19, %t20
  br i1 %t21, label %fbody4, label %fend6
fbody4:
  %t22 = load ptr, ptr %min_dist.6
  %t23 = load i64, ptr %i.11
  %t24 = getelementptr i64, ptr %t22, i64 %t23
  %t25 = load i64, ptr %inf.10
  store i64 %t25, ptr %t24
  %t26 = load ptr, ptr %visited.9
  %t27 = load i64, ptr %i.11
  %t28 = getelementptr i1, ptr %t26, i64 %t27
  store i1 false, ptr %t28
  br label %fpost5
fpost5:
  %t29 = load i64, ptr %i.11
  %t30 = add i64 %t29, 1
  store i64 %t30, ptr %i.11
  br label %fcond3
fend6:
  %t31 = load ptr, ptr %min_dist.6
  %t32 = getelementptr i64, ptr %t31, i64 0
  store i64 0, ptr %t32
  store i64 0, ptr %total_weight.12
  store i64 0, ptr %step.13
  br label %fcond7
fcond7:
  %t33 = load i64, ptr %step.13
  %t34 = load i64, ptr %n.3
  %t35 = icmp slt i64 %t33, %t34
  br i1 %t35, label %fbody8, label %fend10
fbody8:
  %t36 = sub i64 0, 1
  store i64 %t36, ptr %u.14
  %t37 = load i64, ptr %inf.10
  store i64 %t37, ptr %best_d.15
  store i64 0, ptr %i.16
  br label %fcond11
fcond11:
  %t38 = load i64, ptr %i.16
  %t39 = load i64, ptr %n.3
  %t40 = icmp slt i64 %t38, %t39
  br i1 %t40, label %fbody12, label %fend14
fbody12:
  %t41 = load ptr, ptr %visited.9
  %t42 = load i64, ptr %i.16
  %t43 = getelementptr i1, ptr %t41, i64 %t42
  %t44 = load i1, ptr %t43
  %t45 = xor i1 %t44, true
  store i1 %t45, ptr %logic.17
  br i1 %t45, label %logrhs15, label %logdone16
logrhs15:
  %t46 = load ptr, ptr %min_dist.6
  %t47 = load i64, ptr %i.16
  %t48 = getelementptr i64, ptr %t46, i64 %t47
  %t49 = load i64, ptr %t48
  %t50 = load i64, ptr %best_d.15
  %t51 = icmp slt i64 %t49, %t50
  store i1 %t51, ptr %logic.17
  br label %logdone16
logdone16:
  %t52 = load i1, ptr %logic.17
  br i1 %t52, label %then17, label %endif18
then17:
  %t53 = load ptr, ptr %min_dist.6
  %t54 = load i64, ptr %i.16
  %t55 = getelementptr i64, ptr %t53, i64 %t54
  %t56 = load i64, ptr %t55
  store i64 %t56, ptr %best_d.15
  %t57 = load i64, ptr %i.16
  store i64 %t57, ptr %u.14
  br label %endif18
endif18:
  br label %fpost13
fpost13:
  %t58 = load i64, ptr %i.16
  %t59 = add i64 %t58, 1
  store i64 %t59, ptr %i.16
  br label %fcond11
fend14:
  %t60 = load ptr, ptr %visited.9
  %t61 = load i64, ptr %u.14
  %t62 = getelementptr i1, ptr %t60, i64 %t61
  store i1 true, ptr %t62
  %t63 = load i64, ptr %total_weight.12
  %t64 = load i64, ptr %best_d.15
  %t65 = add i64 %t63, %t64
  store i64 %t65, ptr %total_weight.12
  store i64 0, ptr %v.18
  br label %fcond19
fcond19:
  %t66 = load i64, ptr %v.18
  %t67 = load i64, ptr %n.3
  %t68 = icmp slt i64 %t66, %t67
  br i1 %t68, label %fbody20, label %fend22
fbody20:
  %t69 = load ptr, ptr %visited.9
  %t70 = load i64, ptr %v.18
  %t71 = getelementptr i1, ptr %t69, i64 %t70
  %t72 = load i1, ptr %t71
  %t73 = xor i1 %t72, true
  br i1 %t73, label %then23, label %endif24
then23:
  %t74 = load ptr, ptr %orig_x.1
  %t75 = load i64, ptr %u.14
  %t76 = getelementptr i64, ptr %t74, i64 %t75
  %t77 = load i64, ptr %t76
  %t78 = load ptr, ptr %orig_x.1
  %t79 = load i64, ptr %v.18
  %t80 = getelementptr i64, ptr %t78, i64 %t79
  %t81 = load i64, ptr %t80
  %t82 = sub i64 %t77, %t81
  %t83 = call i64 @abs_i64(i64 %t82)
  %t84 = load ptr, ptr %orig_y.2
  %t85 = load i64, ptr %u.14
  %t86 = getelementptr i64, ptr %t84, i64 %t85
  %t87 = load i64, ptr %t86
  %t88 = load ptr, ptr %orig_y.2
  %t89 = load i64, ptr %v.18
  %t90 = getelementptr i64, ptr %t88, i64 %t89
  %t91 = load i64, ptr %t90
  %t92 = sub i64 %t87, %t91
  %t93 = call i64 @abs_i64(i64 %t92)
  %t94 = add i64 %t83, %t93
  store i64 %t94, ptr %d.19
  %t95 = load i64, ptr %d.19
  %t96 = load ptr, ptr %min_dist.6
  %t97 = load i64, ptr %v.18
  %t98 = getelementptr i64, ptr %t96, i64 %t97
  %t99 = load i64, ptr %t98
  %t100 = icmp slt i64 %t95, %t99
  br i1 %t100, label %then25, label %endif26
then25:
  %t101 = load ptr, ptr %min_dist.6
  %t102 = load i64, ptr %v.18
  %t103 = getelementptr i64, ptr %t101, i64 %t102
  %t104 = load i64, ptr %d.19
  store i64 %t104, ptr %t103
  br label %endif26
endif26:
  br label %endif24
endif24:
  br label %fpost21
fpost21:
  %t105 = load i64, ptr %v.18
  %t106 = add i64 %t105, 1
  store i64 %t106, ptr %v.18
  br label %fcond19
fend22:
  br label %fpost9
fpost9:
  %t107 = load i64, ptr %step.13
  %t108 = add i64 %t107, 1
  store i64 %t108, ptr %step.13
  br label %fcond7
fend10:
  %t109 = load i64, ptr %total_weight.12
  call void @BufferGuard__drop(ptr %visited_buf.8)
  call void @BufferGuard__drop(ptr %min_dist_buf.5)
  ret i64 %t109
}

define void @benchmark_100k() inlinehint {
entry:
  %n.1 = alloca i64
  %x.2 = alloca ptr
  %y.3 = alloca ptr
  %rng.4 = alloca i64
  %i.5 = alloca i64
  %t0.6 = alloca i64
  %mst.7 = alloca i64
  %t1.8 = alloca i64
  %elapsed_ms.9 = alloca i64
  store i64 100000, ptr %n.1
  %t1 = load i64, ptr %n.1
  %t2 = getelementptr i64, ptr null, i64 1
  %t3 = ptrtoint ptr %t2 to i64
  %t4 = mul i64 %t1, %t3
  %t5 = call ptr @malloc(i64 %t4)
  store ptr %t5, ptr %x.2
  %t6 = load i64, ptr %n.1
  %t7 = getelementptr i64, ptr null, i64 1
  %t8 = ptrtoint ptr %t7 to i64
  %t9 = mul i64 %t6, %t8
  %t10 = call ptr @malloc(i64 %t9)
  store ptr %t10, ptr %y.3
  store i64 987654321, ptr %rng.4
  store i64 0, ptr %i.5
  br label %fcond1
fcond1:
  %t11 = load i64, ptr %i.5
  %t12 = load i64, ptr %n.1
  %t13 = icmp slt i64 %t11, %t12
  br i1 %t13, label %fbody2, label %fend4
fbody2:
  %t14 = load i64, ptr %rng.4
  %t15 = mul i64 %t14, 1103515245
  %t16 = add i64 %t15, 12345
  %t17 = and i64 %t16, 2147483647
  store i64 %t17, ptr %rng.4
  %t18 = load ptr, ptr %x.2
  %t19 = load i64, ptr %i.5
  %t20 = getelementptr i64, ptr %t18, i64 %t19
  %t21 = load i64, ptr %rng.4
  %t22 = srem i64 %t21, 100000000
  store i64 %t22, ptr %t20
  %t23 = load i64, ptr %rng.4
  %t24 = mul i64 %t23, 1103515245
  %t25 = add i64 %t24, 12345
  %t26 = and i64 %t25, 2147483647
  store i64 %t26, ptr %rng.4
  %t27 = load ptr, ptr %y.3
  %t28 = load i64, ptr %i.5
  %t29 = getelementptr i64, ptr %t27, i64 %t28
  %t30 = load i64, ptr %rng.4
  %t31 = srem i64 %t30, 100000000
  store i64 %t31, ptr %t29
  br label %fpost3
fpost3:
  %t32 = load i64, ptr %i.5
  %t33 = add i64 %t32, 1
  store i64 %t33, ptr %i.5
  br label %fcond1
fend4:
  %t34 = load i64, ptr %n.1
  %t35 = call i32 (ptr, ...) @printf(ptr @.str.1, i64 %t34)
  %t36 = call i64 @clock()
  store i64 %t36, ptr %t0.6
  %t37 = load ptr, ptr %x.2
  %t38 = load ptr, ptr %y.3
  %t39 = load i64, ptr %n.1
  %t40 = call i64 @solve_manhattan_mst(ptr %t37, ptr %t38, i64 %t39)
  store i64 %t40, ptr %mst.7
  %t41 = call i64 @clock()
  store i64 %t41, ptr %t1.8
  %t42 = load i64, ptr %t1.8
  %t43 = load i64, ptr %t0.6
  %t44 = sub i64 %t42, %t43
  store i64 %t44, ptr %elapsed_ms.9
  %t45 = load i64, ptr %mst.7
  %t46 = load i64, ptr %elapsed_ms.9
  %t47 = call i32 (ptr, ...) @printf(ptr @.str.2, i64 %t45, i64 %t46)
  %t48 = load ptr, ptr %x.2
  call void @free(ptr %t48)
  %t49 = load ptr, ptr %y.3
  call void @free(ptr %t49)
  ret void
}

define i32 @main() {
entry:
  %n3.1 = alloca i64
  %x3.2 = alloca ptr
  %y3.3 = alloca ptr
  %mst3.4 = alloca i64
  %n4.5 = alloca i64
  %x4.6 = alloca ptr
  %y4.7 = alloca ptr
  %mst4.8 = alloca i64
  %t1 = call i32 (ptr, ...) @printf(ptr @.str.3)
  %t2 = call i32 (ptr, ...) @printf(ptr @.str.4)
  %t3 = call i32 (ptr, ...) @printf(ptr @.str.5)
  store i64 3, ptr %n3.1
  %t4 = load i64, ptr %n3.1
  %t5 = getelementptr i64, ptr null, i64 1
  %t6 = ptrtoint ptr %t5 to i64
  %t7 = mul i64 %t4, %t6
  %t8 = call ptr @malloc(i64 %t7)
  store ptr %t8, ptr %x3.2
  %t9 = load i64, ptr %n3.1
  %t10 = getelementptr i64, ptr null, i64 1
  %t11 = ptrtoint ptr %t10 to i64
  %t12 = mul i64 %t9, %t11
  %t13 = call ptr @malloc(i64 %t12)
  store ptr %t13, ptr %y3.3
  %t14 = load ptr, ptr %x3.2
  %t15 = getelementptr i64, ptr %t14, i64 0
  store i64 0, ptr %t15
  %t16 = load ptr, ptr %y3.3
  %t17 = getelementptr i64, ptr %t16, i64 0
  store i64 0, ptr %t17
  %t18 = load ptr, ptr %x3.2
  %t19 = getelementptr i64, ptr %t18, i64 1
  store i64 1, ptr %t19
  %t20 = load ptr, ptr %y3.3
  %t21 = getelementptr i64, ptr %t20, i64 1
  store i64 2, ptr %t21
  %t22 = load ptr, ptr %x3.2
  %t23 = getelementptr i64, ptr %t22, i64 2
  store i64 2, ptr %t23
  %t24 = load ptr, ptr %y3.3
  %t25 = getelementptr i64, ptr %t24, i64 2
  store i64 1, ptr %t25
  %t26 = load ptr, ptr %x3.2
  %t27 = load ptr, ptr %y3.3
  %t28 = load i64, ptr %n3.1
  %t29 = call i64 @solve_manhattan_mst(ptr %t26, ptr %t27, i64 %t28)
  store i64 %t29, ptr %mst3.4
  %t30 = load i64, ptr %mst3.4
  %t31 = call i32 (ptr, ...) @printf(ptr @.str.6, i64 %t30)
  %t32 = load ptr, ptr %x3.2
  call void @free(ptr %t32)
  %t33 = load ptr, ptr %y3.3
  call void @free(ptr %t33)
  store i64 4, ptr %n4.5
  %t34 = load i64, ptr %n4.5
  %t35 = getelementptr i64, ptr null, i64 1
  %t36 = ptrtoint ptr %t35 to i64
  %t37 = mul i64 %t34, %t36
  %t38 = call ptr @malloc(i64 %t37)
  store ptr %t38, ptr %x4.6
  %t39 = load i64, ptr %n4.5
  %t40 = getelementptr i64, ptr null, i64 1
  %t41 = ptrtoint ptr %t40 to i64
  %t42 = mul i64 %t39, %t41
  %t43 = call ptr @malloc(i64 %t42)
  store ptr %t43, ptr %y4.7
  %t44 = load ptr, ptr %x4.6
  %t45 = getelementptr i64, ptr %t44, i64 0
  store i64 0, ptr %t45
  %t46 = load ptr, ptr %y4.7
  %t47 = getelementptr i64, ptr %t46, i64 0
  store i64 0, ptr %t47
  %t48 = load ptr, ptr %x4.6
  %t49 = getelementptr i64, ptr %t48, i64 1
  store i64 0, ptr %t49
  %t50 = load ptr, ptr %y4.7
  %t51 = getelementptr i64, ptr %t50, i64 1
  store i64 10, ptr %t51
  %t52 = load ptr, ptr %x4.6
  %t53 = getelementptr i64, ptr %t52, i64 2
  store i64 10, ptr %t53
  %t54 = load ptr, ptr %y4.7
  %t55 = getelementptr i64, ptr %t54, i64 2
  store i64 0, ptr %t55
  %t56 = load ptr, ptr %x4.6
  %t57 = getelementptr i64, ptr %t56, i64 3
  store i64 10, ptr %t57
  %t58 = load ptr, ptr %y4.7
  %t59 = getelementptr i64, ptr %t58, i64 3
  store i64 10, ptr %t59
  %t60 = load ptr, ptr %x4.6
  %t61 = load ptr, ptr %y4.7
  %t62 = load i64, ptr %n4.5
  %t63 = call i64 @solve_manhattan_mst(ptr %t60, ptr %t61, i64 %t62)
  store i64 %t63, ptr %mst4.8
  %t64 = load i64, ptr %mst4.8
  %t65 = call i32 (ptr, ...) @printf(ptr @.str.7, i64 %t64)
  %t66 = load ptr, ptr %x4.6
  call void @free(ptr %t66)
  %t67 = load ptr, ptr %y4.7
  call void @free(ptr %t67)
  call void @benchmark_100k()
  %t68 = call i32 (ptr, ...) @printf(ptr @.str.8)
  ret i32 0
}

