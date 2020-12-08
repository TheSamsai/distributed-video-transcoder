# distributed-video-transcoder
Project for Distributes Systems course aiming to implement a distributed system
for video transcoding

## Building

The software consists of two components written in the Rust programming
language. They are found under the ```node-server``` and ```job-server```
directories.

In order to build the software, you need the Rust compiler and cargo package
manager. You can install them from https://rustup.rs/.

The node-server connects trough ssh so ```RSYNC_USER``` in ```job-server/run.sh```
needs to be able to authenticate trough ssh's public key authectication.

### Building job-server

The job-server requires the latest nightly version of Rust which can be
installed through rustup as follows:

``` sh
rustup install nightly
```

Then in the job-server directory run the following commands:

``` sh
rustup override set nightly

cargo build --release
```

At this point the job-server can be launched using the ```run.sh``` file.

The parameters of the job-server are set in the ```run.sh``` file. Tasks can be
submitted by moving video files into the ```incoming/``` directory.

### Building node-server

The node-server is the worker component of the system.

Node server needs **rsync** and **ffmpeg** to be installed.

Then in the node-server directory run the following commands:

``` sh
cargo build --release
```

At this point the job-server can be launched as follows:

``` sh
cargo run --release http://<ip-address-of-the-job-server>:8000
```

## System description and architecture

The system implements a video transcoding service that can distribute video
encoding tasks to a number of different systems to process a large number of
videos in parallel. The particular use-case the system has been designed for is
to compress large video archives to take advantage of more modern, but also more
encoding-intensive video encoders, like VP9 and HEVC/H265 or AV1.

The system is implemented using a leader/follower architecture, where a single
"job server" is used to keep track of files and hand out tasks to "worker
nodes". Any number of worker nodes can register themselves with the job server
and inform themselves as being alive by sending a regular "heartbeat" signal to
the server and check in with the server for new tasks.

Communication between the workers and the job server is handled using an
HTTP-based API and file transfer to and from the job server is handled via SFTP
(using rsync). Communication is strictly two-way between the job server and the
worker node, and is initiated by the worker node. No communication between
workers is implemented. Encoding tasks are handled by the workers using ffmpeg. 

The system could be used as a base for an advanced "encoding farm" solution and
with a decent front-end be expanded into a video encoding web application.

## Consistency, synchronization and fault-tolerance

Due to the hierarchical architecture, the entire system a single point of
failure, which is the job server. However, this is partially accounted for by
keeping the leader node's software implementation simple, thus largely limiting
potential failures to hardware faults. The system can survive any number of
silent failures from workers and has a system in place to reallocate work if a
worker fails to check in regularly.It, however, cannot protect against
arbitrary failures or malicious workers.

The job server can recover from crashes by being restarted. Since tasks are
stored on the filesystem (in the form of an incoming/completed directory, where
video files are stored), it can continue operating close to where it crashed.
However, all worker information and allocated tasks will be lost and in-progress
tasks must be restarted.

The largest failure point in workers is the calls to spawn subprocesses for rsync
and ffmpeg, as failures in these processed are only noticed after the process has finished
with its return code. These failures hard to handle automatically because they are 
usually caused by configurations problems in the worker's operating system 
(dependency not installed, wrong version, etc.). Currently, if the worker notices that
the conversion process has failed, the sub processes outputs are logged with a timestamp.
This information is also sent to the job server for centralized logging.
Node also takes it self down so as a way to not cause continuous problems.

Consistency concerns are minimal due to the leader being responsible for
managing the majority of the system state. The workers are each allocated
independent tasks, which means they don't need to worry about managing shared
state. This also means that synchronization concerns are minimal, with most of
the synchronization concerns being handled at the job server and by the
filesystem.

## Performance and scaling

Since all of the encoding tasks are independent, the system can theoretically be
scaled to any number of worker nodes with more or less a linear performance
improvement. This means that the problem is "embarrassingly parallel".

In practice this isn't entirely true, however. The biggest limitations to the
performance of the system come from the limited network bandwidth available to
the job server and the performance characteristics of the storage medium of the 
job server. The main bottleneck is simply the job server. Particularly a large
number of worker nodes could relatively quickly overwhelm the job server's
network bandwidth in file downloads and uploads.

The system also scales by the number of encoding tasks given, which means that
it scales poorly if given a small number of very long encoding tasks. The
current design of the system means that nodes cannot split single encoding tasks
into multiple smaller encoding tasks and thus collaborate with each other. A
single encoding task always goes to a single worker and that worker's speed
determines the time in which the task will be completed.

## Improving the system

The system could be improved in a number of different ways. One of the biggest
improvements would be to solve the problem of a single job server having its
bandwidth starved by too many workers. This problem could be solved by either
adding more hierarchy to the system by allowing job servers to have other job
servers acting as "delegating workers". This means that a single job server
could distribute its tasks to a number of other job servers, which themselves
either have more delegating workers or normal worker nodes. Alternatively the
system architecture could be decentralized and task management and file sharing
could utilize a peer-to-peer model. This would require a significant
re-architecting of the system.

The system scaling could also be improved to better deal with a smaller number
of very big tasks. Large video files could, when possible, be allocated to a
number of workers to process only a part of the files and then the job server
could stitch the output of multiple of these workers together. The main problem
with this is that only certain video container formats allow files to be
appended without a full re-encode and heuristics would need to be developed to
only partition video files of certain length so that tasks aren't needlessly
being partitioned. Furthermore it would increase the amount of logic running on
the job server to keep track of which files have been partitioned, which worker
is responsible for which partition and then being able to put the partitions
back together in the correct order.

The last area of improvements would be to fail-tolerance. Currently the system
is vulnerable to two types of failures: failures to the job server and arbitrary
failures to workers. A misbehaving worker could push corrupted video files to
the job server and the server wouldn't be able to find this out.

The failures to the job server would require major re-architecting of the
system. Firstly, the file storage would need to be separated from the job
server and in order to completely eliminate single points of failure, files
would need to be stored in a distributed fashion between multiple active
nodes. Then either a chain of command or an election algorithm could be
implemented to allow another node to be selected as the leader in case of the
current leader becomes inaccessible. All of this also requires that each node
can communicate with every other node, which means improving the communication
code of the system.

Worker failures could be addressed with less effort. The easiest way to prevent
N arbitrarily failing worker nodes would be to ensure a single task is always
given to 2N + 1 workers, and when the workers submit their results, the files
can be checksummed and the version of the file with most identical checksums
would be accepted. This, however, assumes that each worker uses identical ffmpeg
software to ensure the encoding process deterministically produces the same
output every time. If multiple files can be "correct" but vary slightly in their
checksums, establishing consensus computationally would be non-trivial.

## What was learned

The system uses relatively few advanced distributed systems features and is
technologically quite simple, and builds on existing components (rsync, ffmpeg,
inotify). This means that the majority of learning has been in finding out how
each of these components works and how they can be combined together to form the
system.

However, designing this system did involve having to plan for some failure cases
and even in its current state the system involves a little bit of asynchronity
from both the job server and the worker, which we've had to manage correctly. In
the case of the job server, this has mainly involved correct handling of mutexes
and in the case of the worker node, proper application of the async/await
functionality of the Rust language to send out the heartbeat signal regularly
while waiting for files to be processed.

Designing the system has also revealed to us many of the current shortcomings of
the design and has required us to at least think about how these problems could
be solved. As this document shows, we already have some ideas in mind and with
more time could begin to explore these ideas to establish a better distributed
system on top of the current design.
