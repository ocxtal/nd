
# global settings
set terminal png size 640 480 font "Helvetica,12"
set key autotitle columnhead left top


# first plot for formatting throughput
set output "results/format.png"

# --width in [4, 65536]; throughput in [1MB/s, 10GB/s]
set logscale x; set xrange [1:100000]; set xlabel "width (B/line)"
set logscale y; set yrange [1:10000]; set ylabel "throughput (MB/s)"

plot for [i = 2:5] "results/format.tsv" using 1:i with linespoint


# second plot for parsing throughput
set output "results/parse.png"

# --width in [4, 65536]; throughput in [1MB/s, 10GB/s]
set logscale x; set xrange [1:100000]; set xlabel "width (B/line)"
set logscale y; set yrange [1:10000]; set ylabel "throughput (MB/s)"

plot for [i = 2:3] "results/parse.tsv" using 1:i with linespoint
